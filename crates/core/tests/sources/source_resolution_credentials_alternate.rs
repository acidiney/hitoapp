use crate::{resolution_failover_fixture as protocol, resolution_musical_fixture as saved};
use hito_core::test_support::hang_guard::HANG_CEILING;

use futures_util::FutureExt;
use hito_core::{
    Credentials, Error, InstallPlugin, OfferPluginService, OpenedAudio, ResolveSource,
    SourcePreferenceEntry, SourceResolution, UpdateSourcePreferences,
};
use hito_plugin_sdk::{Capability, Manifest, PROTOCOL_VERSION, SEARCH_PATH, SearchRequest};
use hito_testkit::fake_plugin::{FakePlugin, Gate, Reply};
use protocol::{Event, Plan};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::task::JoinSet;
use uuid::Uuid;

#[tokio::test]
async fn a_provider_search_credential_failure_keeps_the_next_ready_provider_available() {
    let primary = saved::ProviderServer::start().await;
    let previous_audio = saved::ProviderServer::start().await;
    let mut failing = FakePlugin::builder(Manifest {
        id: "org.hito.source-credentials-first-fixture".into(),
        name: "Owned first credentials service".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Search,
            Capability::Sources,
            Capability::Metadata,
        ]
        .into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .post(SEARCH_PATH, |request| {
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        assert!(!request.has_authorization());
        Reply::Status(axum::http::StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    failing.set_hold_limit(Duration::from_secs(4));
    let gate = failing.hold_next(SEARCH_PATH);
    let mut healthy = None;
    let mut requests = JoinSet::new();
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            &primary,
            &previous_audio,
            &failing,
            &gate,
            &mut healthy,
            &mut requests,
        )))
        .catch_unwind(),
    )
    .await;
    gate.open();
    requests.abort_all();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await
    .unwrap();
    let failing_stopped = failing.finish().await;
    let healthy_stopped = if let Some(provider) = healthy.as_mut() {
        provider.finish().await
    } else {
        true
    };
    primary.shutdown().await;
    previous_audio.shutdown().await;
    assert!(failing_stopped && healthy_stopped);
    let resolution = match result {
        Ok(Ok(resolution)) => resolution,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned healthy alternate must settle within its deadline"),
    };
    assert_eq!(
        serde_json::to_value(resolution).unwrap()["outcome"],
        json!("ready")
    );
}

async fn exercise(
    primary: &saved::ProviderServer,
    previous_audio: &saved::ProviderServer,
    failing: &FakePlugin,
    gate: &Gate,
    healthy: &mut Option<protocol::Provider>,
    requests: &mut JoinSet<Result<SourceResolution, Error>>,
) -> SourceResolution {
    let mut owned = Box::pin(saved::SavedRecording::prepare(primary, previous_audio)).await;
    owned
        .prepare_private_title("Private alternate title override")
        .await;
    owned
        .hito
        .plugins
        .remove(
            &owned.actor,
            &owned.audio_installation.id,
            owned.audio_installation.revision,
        )
        .await
        .unwrap();
    let service = owned
        .hito
        .plugins
        .offer_service(
            &owned.actor,
            OfferPluginService {
                name: "Owned first credentials service".into(),
                base_url: failing.url.clone(),
            },
        )
        .await
        .unwrap();
    let first = owned
        .hito
        .plugins
        .install(
            &owned.actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    *healthy = Some(protocol::Provider::start(Plan::FailedThenReady, owned.original.clone()).await);
    let healthy = healthy.as_ref().unwrap();
    let second = healthy.connect(&owned.hito, &owned.actor).await;
    let connections = owned.hito.plugins.connections(&owned.actor).await.unwrap();
    let eligible = connections
        .installations
        .iter()
        .filter(|installation| {
            installation
                .manifest
                .capabilities
                .contains(&Capability::Search)
                && installation
                    .manifest
                    .capabilities
                    .contains(&Capability::Sources)
        })
        .map(|installation| installation.id.as_str())
        .collect::<Vec<_>>();
    assert!(
        eligible.len() == 2
            && eligible.contains(&first.id.as_str())
            && eligible.contains(&second.id.as_str())
    );
    let preferences = owned
        .hito
        .catalog
        .source_preferences(&owned.actor)
        .await
        .unwrap();
    let preferences = owned
        .hito
        .catalog
        .update_source_preferences(
            &owned.actor,
            UpdateSourcePreferences {
                revision: preferences.revision,
                fallback: false,
                entries: vec![
                    SourcePreferenceEntry::Provider {
                        installation_id: first.id.clone(),
                    },
                    SourcePreferenceEntry::Provider {
                        installation_id: second.id.clone(),
                    },
                ],
            },
        )
        .await
        .unwrap();
    let preferences = serde_json::to_value(preferences).unwrap();
    let choice = owned
        .hito
        .catalog
        .source_choice(&owned.actor, &owned.recording)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(choice).unwrap()["mode"], "automatic");
    owned
        .hito
        .identity
        .create_user(
            &owned.actor,
            Credentials {
                username: "source-alternate-peer".into(),
                password: "owned source alternate peer password".into(),
            },
        )
        .await
        .unwrap();
    let session = owned
        .hito
        .identity
        .login(Credentials {
            username: "source-alternate-peer".into(),
            password: "owned source alternate peer password".into(),
        })
        .await
        .unwrap();
    let peer = owned
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(
        owned
            .hito
            .plugins
            .connections(&peer)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    assert!(matches!(
        owned
            .hito
            .catalog
            .source_choice(&peer, &owned.recording)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        owned
            .hito
            .catalog
            .resolve_source_until(
                &peer,
                &owned.recording,
                ResolveSource {
                    request_id: Uuid::new_v4()
                },
                std::future::pending(),
            )
            .await,
        Err(Error::NotFound)
    ));
    let peer_preferences =
        serde_json::to_value(owned.hito.catalog.source_preferences(&peer).await.unwrap()).unwrap();
    assert_eq!(healthy.events().await, vec![Event::Manifest]);
    assert!(failing.requests_for(SEARCH_PATH).is_empty());
    let request_id = Uuid::new_v4();
    let hito = owned.hito.clone();
    let actor = owned.actor.clone();
    let recording = owned.recording.clone();
    requests.spawn(async move {
        hito.catalog
            .resolve_source_until(
                &actor,
                &recording,
                ResolveSource { request_id },
                std::future::pending(),
            )
            .await
    });
    assert!(gate.entered_within(Duration::from_secs(1)).await);
    assert_eq!(failing.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(healthy.events().await, vec![Event::Manifest]);
    assert!(requests.try_join_next().is_none());
    gate.release();
    let resolution = requests.join_next().await.unwrap().unwrap().unwrap();
    assert_eq!(resolution.request_id, request_id);
    assert_eq!(resolution.recording_id, owned.recording);
    assert_eq!(resolution.sources.len(), 1);
    assert!(resolution.sources[0].ready);
    assert_eq!(resolution.sources[0].recording_id, owned.recording);
    assert_eq!(resolution.sources[0].provider, "plugin");
    assert_eq!(
        resolution.sources[0].provider_label.as_deref(),
        Some("Owned resolution failover provider")
    );
    assert_eq!(
        serde_json::to_value(resolution.sources[0].match_kind).unwrap(),
        "verified"
    );
    let expected = vec![
        Event::Manifest,
        Event::Search,
        Event::FailedDetail,
        Event::FailedSources,
        Event::ReadyDetail,
        Event::ReadySources,
        Event::Metadata,
    ];
    assert_eq!(healthy.events().await, expected);
    assert_eq!(failing.requests().len(), 2);
    assert!(failing.health_requests().is_empty());
    assert_eq!(failing.unexpected(), 0);
    let replay = owned
        .hito
        .catalog
        .resolve_source_until(
            &owned.actor,
            &owned.recording,
            ResolveSource { request_id },
            std::future::pending(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(&resolution).unwrap()
    );
    assert!(matches!(
        owned
            .hito
            .catalog
            .resolve_source_until(
                &peer,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        owned.hito.catalog.sources(&peer, &owned.recording).await,
        Err(Error::NotFound)
    ));
    assert_eq!(healthy.events().await, expected);
    assert_eq!(failing.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(
        serde_json::to_value(
            owned
                .hito
                .catalog
                .source_choice(&owned.actor, &owned.recording)
                .await
                .unwrap()
        )
        .unwrap()["mode"],
        "automatic"
    );
    let metadata = owned
        .hito
        .catalog
        .recording_metadata(&owned.actor, &owned.recording, Default::default())
        .await
        .unwrap();
    assert!(metadata.observations.items.iter().all(|observation| {
        observation.provider_recording_id.as_deref() != Some(protocol::FAILED)
            && observation.provider != "org.hito.source-credentials-first-fixture"
    }));
    assert_eq!(
        metadata
            .observations
            .items
            .iter()
            .filter(
                |observation| observation.provider_recording_id.as_deref() == Some(protocol::READY)
            )
            .count(),
        1
    );
    owned.assert_preserved().await;
    owned.assert_original_preserved();
    let OpenedAudio::Content(mut audio) = owned
        .hito
        .playback
        .open_audio(&owned.actor, &resolution.sources[0].id, Default::default())
        .await
        .unwrap()
    else {
        panic!("the alternate must supply owned audio")
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, owned.original);
    let mut after_audio = expected;
    after_audio.push(Event::Audio);
    assert_eq!(healthy.events().await, after_audio);
    assert_eq!(failing.requests_for(SEARCH_PATH).len(), 1);
    assert!(matches!(
        owned
            .hito
            .downloads
            .status(&owned.actor, &owned.recording)
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        serde_json::to_value(
            owned
                .hito
                .catalog
                .source_preferences(&owned.actor)
                .await
                .unwrap()
        )
        .unwrap(),
        preferences
    );
    assert_eq!(
        serde_json::to_value(owned.hito.catalog.source_preferences(&peer).await.unwrap()).unwrap(),
        peer_preferences
    );
    assert!(
        owned
            .hito
            .plugins
            .connections(&peer)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    owned.assert_preserved().await;
    owned.assert_original_preserved();
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        assert!(previous_audio.requests(path).await.is_empty());
    }
    let receipt = serde_json::to_string(&resolution).unwrap();
    for private in [
        "spotify/owned-recording",
        protocol::READY,
        "Private alternate title override",
        primary.url.as_str(),
        previous_audio.url.as_str(),
        failing.url.as_str(),
    ] {
        assert!(!receipt.contains(private));
    }
    resolution
}
