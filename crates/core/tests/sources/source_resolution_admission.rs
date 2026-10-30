#[path = "../fixtures/resolution_revocation.rs"]
mod provider;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    Actor, AdoptProviderRecording, Error, InstallPlugin, OfferPluginService, PluginInstallation,
    ResolveSource, Services, SourceResolution, SourceResolutionOutcome,
};
use provider::{Event, Provider};
use std::{collections::HashSet, panic::AssertUnwindSafe, time::Duration};
use tokio::task::{JoinHandle, JoinSet};
use uuid::Uuid;

#[tokio::test]
async fn a_busy_account_rejects_resolution_callers_without_creating_observer_aliases() {
    let (directory, hito, actor, _, _) = support::library("resolution-admission").await;
    let provider = Provider::start().await;
    let mut owner = None;
    let mut excess = JoinSet::new();
    let result = AssertUnwindSafe(exercise(&hito, &actor, &provider, &mut owner, &mut excess))
        .catch_unwind()
        .await;
    excess.abort_all();
    while excess.join_next().await.is_some() {}
    if let Some(owner) = owner {
        owner.abort();
        let _ = owner.await;
    }
    let stopped = provider.close().await;
    drop(hito);
    drop(directory);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    assert!(stopped, "the owned provider server must stop and join");
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    provider: &Provider,
    owner: &mut Option<JoinHandle<hito_core::Result<SourceResolution>>>,
    excess: &mut JoinSet<hito_core::Result<SourceResolution>>,
) {
    let primary = connect(hito, actor, provider.metadata_url(), None).await;
    let _audio = connect(
        hito,
        actor,
        provider.audio_url(),
        Some("owned-resolution-private-token".into()),
    )
    .await;
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &primary.id,
            AdoptProviderRecording {
                installation_revision: primary.revision,
                provider_recording_id: "owned/primary".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        hito.catalog
            .sources(actor, &recording.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(provider.events().await.is_empty());
    let request_id = Uuid::new_v4();
    *owner = Some({
        let catalog = hito.catalog.clone();
        let actor = actor.clone();
        let recording = recording.id.clone();
        tokio::spawn(async move {
            catalog
                .resolve_source_until(
                    &actor,
                    &recording,
                    ResolveSource { request_id },
                    std::future::pending(),
                )
                .await
        })
    });
    tokio::time::timeout(Duration::from_secs(3), provider.candidate_sources_entered())
        .await
        .expect("the first resolver must own its held candidate HTTP exchange");
    assert_eq!(
        provider.events().await,
        [Event::Search, Event::DetailA, Event::SourcesA]
    );
    let rejected = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    for request_id in [request_id, request_id].into_iter().chain(rejected) {
        let catalog = hito.catalog.clone();
        let actor = actor.clone();
        let recording = recording.id.clone();
        excess.spawn(async move {
            catalog
                .resolve_source_until(
                    &actor,
                    &recording,
                    ResolveSource { request_id },
                    std::future::pending(),
                )
                .await
        });
    }
    let returned = tokio::time::timeout(Duration::from_secs(1), async {
        let mut returned = 0;
        while let Some(joined) = excess.join_next().await {
            let result = joined.expect("each rejected resolver must remain joined");
            assert!(matches!(result, Err(Error::Conflict(_))),
                "each excess same-account caller must return controlled Busy while the owner is held");
            returned += 1;
        }
        returned
    }).await.expect("all excess callers must return Busy before the held provider is released");
    assert_eq!(returned, 6);
    assert_eq!(
        provider.events().await,
        [Event::Search, Event::DetailA, Event::SourcesA],
        "rejected callers must not dispatch additional provider work"
    );
    provider.release_candidate_sources();
    let joined = tokio::time::timeout(Duration::from_secs(3), owner.as_mut().unwrap())
        .await
        .expect("the admitted owner must finish after its provider is released");
    owner.take();
    let accepted = joined.expect("the owner task must join").unwrap();
    assert!(matches!(accepted.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(accepted.request_id, request_id);
    assert!(accepted.recording_id == recording.id);
    assert_eq!(accepted.sources.len(), 1);
    assert!(accepted.sources[0].ready);
    let source_id = accepted.sources[0].id.clone();
    let catalog = hito.catalog.clone();
    let replay = tokio::time::timeout(
        Duration::from_secs(1),
        catalog.resolve_source_until(
            actor,
            &recording.id,
            ResolveSource { request_id },
            std::future::pending(),
        ),
    )
    .await
    .expect("the owner's exact request must replay once its call has settled")
    .unwrap();
    assert_eq!(replay.resolution_id, accepted.resolution_id);
    assert!(matches!(replay.outcome, SourceResolutionOutcome::Ready));
    assert!(
        replay
            .sources
            .iter()
            .any(|source| source.id == source_id && source.ready)
    );
    let mut identities = HashSet::from([accepted.resolution_id]);
    for request_id in rejected {
        let catalog = hito.catalog.clone();
        let retry = tokio::time::timeout(
            Duration::from_secs(1),
            catalog.resolve_source_until(
                actor,
                &recording.id,
                ResolveSource { request_id },
                std::future::pending(),
            ),
        )
        .await
        .expect("a previously rejected new request must be admitted after the owner settles")
        .unwrap();
        assert!(matches!(retry.outcome, SourceResolutionOutcome::Ready));
        assert_eq!(retry.request_id, request_id);
        assert!(retry.recording_id == recording.id);
        assert!(
            identities.insert(retry.resolution_id),
            "Busy must not persist a new UUID as an alias of the owner's durable receipt"
        );
        assert!(
            retry
                .sources
                .iter()
                .any(|source| source.id == source_id && source.ready)
        );
    }
    assert_eq!(
        provider.events().await,
        [
            Event::Search,
            Event::DetailA,
            Event::SourcesA,
            Event::DetailB,
            Event::SourcesB,
        ],
        "ready source use and committed receipt replay must not repeat provider requests"
    );
    assert!(
        provider.authenticated(),
        "provider exchanges must retain their approved credential"
    );
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    base_url: String,
    bearer_token: Option<String>,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned admission fixture".into(),
                base_url,
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token,
            },
        )
        .await
        .unwrap()
}
