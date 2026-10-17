#[path = "fixtures/source_preparation_history.rs"]
mod fixture;
mod support;

use hito_core::{
    Actor, AdoptProviderRecording, Credentials, Error, InstallPlugin, OfferPluginService,
    PluginInstallation, PrepareSource, Services, SourcePreparationPhase, SourcePreparationReceipt,
};
use std::collections::HashSet;
use uuid::Uuid;

#[tokio::test]
async fn retained_receipts_allow_continued_preparation_without_duplicate_pending_work() {
    let provider = fixture::Provider::start().await;
    let (_directory, hito, admin, _, _) = support::library("preparation-history").await;
    let service = hito
        .plugins
        .offer_service(
            &admin,
            OfferPluginService {
                name: "Owned preparation history provider".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let mut actors = vec![admin.clone()];
    for account in 1..10 {
        let credentials = || Credentials {
            username: format!("preparation-account-{account}"),
            password: "owned preparation history password".into(),
        };
        hito.identity
            .create_user(&admin, credentials())
            .await
            .unwrap();
        let session = hito.identity.login(credentials()).await.unwrap();
        actors.push(hito.identity.authenticate(&session.token).await.unwrap());
    }
    let mut targets = Vec::new();
    let mut installations = Vec::new();
    for actor in &actors {
        let installation = hito
            .plugins
            .install(
                actor,
                InstallPlugin {
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: Some(fixture::TOKEN.into()),
                },
            )
            .await
            .unwrap();
        targets.push(adopt_target(&hito, actor, &installation, fixture::RECORDING).await);
        installations.push(installation);
    }
    assert_eq!(
        targets
            .iter()
            .map(|(recording, _)| recording)
            .collect::<HashSet<_>>()
            .len(),
        10
    );
    assert_eq!(
        targets
            .iter()
            .map(|(_, source)| source)
            .collect::<HashSet<_>>()
            .len(),
        10
    );

    let mut histories = Vec::new();
    for (account, (actor, (recording, source))) in actors.iter().zip(&targets).enumerate() {
        let mut history: Vec<SourcePreparationReceipt> = Vec::new();
        for request in 1..=411 {
            let request_id = Uuid::new_v4();
            let receipt = hito
                .catalog
                .prepare_source(actor, recording, source, PrepareSource { request_id })
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "account {account} request {request} must retain a fresh receipt for its existing queued preparation: {error}"
                    )
                });
            assert_eq!(receipt.request_id, request_id);
            assert_eq!(receipt.recording_id, *recording);
            assert_eq!(receipt.source_id, *source);
            if let Some(first) = history.first() {
                assert_eq!(receipt.preparation_id, first.preparation_id);
            }
            history.push(receipt);
        }
        histories.push(history);
    }
    assert_eq!(histories.iter().map(Vec::len).sum::<usize>(), 4110);
    assert_eq!(
        histories
            .iter()
            .map(|history| history[0].preparation_id)
            .collect::<HashSet<_>>()
            .len(),
        10
    );

    for ((actor, (recording, source)), history) in actors.iter().zip(&targets).zip(&histories) {
        for saved in [history.first().unwrap(), history.last().unwrap()] {
            let replay = hito
                .catalog
                .prepare_source(
                    actor,
                    recording,
                    source,
                    PrepareSource {
                        request_id: saved.request_id,
                    },
                )
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(replay).unwrap(),
                serde_json::to_value(saved).unwrap()
            );
        }
        let status = hito
            .catalog
            .source_preparation(actor, recording, source)
            .await
            .unwrap();
        assert_eq!(status.preparation_id, history[0].preparation_id);
        assert_eq!(status.recording_id, *recording);
        assert_eq!(status.source_id, *source);
        assert!(matches!(status.phase, SourcePreparationPhase::Queued));
        assert!(matches!(
            hito.downloads.status(actor, recording).await,
            Err(Error::NotFound)
        ));
        let catalog = hito
            .catalog
            .recordings(actor, Default::default())
            .await
            .unwrap();
        assert_eq!(catalog.total, 1);
        assert_eq!(catalog.items[0].id, *recording);
        assert!(!catalog.items[0].available);
        let sources = hito.catalog.sources(actor, recording).await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, *source);
        assert!(!sources[0].ready);
    }
    for (account, actor) in actors.iter().enumerate() {
        let (foreign_recording, foreign_source) = &targets[(account + 1) % actors.len()];
        assert!(matches!(
            hito.catalog
                .source_preparation(actor, foreign_recording, foreign_source)
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.catalog.sources(actor, foreign_recording).await,
            Err(Error::NotFound)
        ));
    }

    let (recording, source) = adopt_target(
        &hito,
        &actors[0],
        &installations[0],
        fixture::OTHER_RECORDING,
    )
    .await;
    let request_id = Uuid::new_v4();
    let fresh = hito
        .catalog
        .prepare_source(
            &actors[0],
            &recording,
            &source,
            PrepareSource { request_id },
        )
        .await
        .expect("retained request history must leave room for another legitimate pending target");
    assert_eq!(fresh.request_id, request_id);
    assert_eq!(fresh.recording_id, recording);
    assert_eq!(fresh.source_id, source);
    assert!(
        histories
            .iter()
            .all(|history| history[0].preparation_id != fresh.preparation_id)
    );
    let status = hito
        .catalog
        .source_preparation(&actors[0], &recording, &source)
        .await
        .unwrap();
    assert_eq!(status.preparation_id, fresh.preparation_id);
    assert!(matches!(status.phase, SourcePreparationPhase::Queued));
    assert!(matches!(
        hito.downloads.status(&actors[0], &recording).await,
        Err(Error::NotFound)
    ));
    assert_eq!(provider.work_counts(), (0, 0, 0, 0));
}

async fn adopt_target(
    hito: &Services,
    actor: &Actor,
    installation: &PluginInstallation,
    provider_recording: &str,
) -> (String, String) {
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: provider_recording.into(),
            },
        )
        .await
        .unwrap();
    assert!(!recording.available);
    let mut sources = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    let source = sources.remove(0);
    assert_eq!(source.recording_id, recording.id);
    assert!(!source.ready);
    (recording.id, source.id)
}
