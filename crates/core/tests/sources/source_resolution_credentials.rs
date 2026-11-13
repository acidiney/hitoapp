use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_core::test_support::resolution_unavailable as fixture;

use futures_util::FutureExt;
use hito_core::{
    Credentials, Error, InstallPlugin, OfferPluginService, ResolveSource, SourceResolution,
};
use hito_plugin_sdk::{Capability, Manifest, PROTOCOL_VERSION, SEARCH_PATH, SearchRequest};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn a_provider_search_credential_failure_is_unavailable_instead_of_a_musical_mismatch() {
    let primary = fixture::ProviderServer::start().await;
    let previous_audio = fixture::ProviderServer::start().await;
    let mut failing = FakePlugin::builder(Manifest {
        id: "org.hito.source-credentials-fixture".into(),
        name: "Owned source credentials service".into(),
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
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(&primary, &previous_audio, &failing))).catch_unwind(),
    )
    .await;
    let stopped = failing.finish().await;
    primary.shutdown().await;
    previous_audio.shutdown().await;
    assert!(stopped);
    let resolution = match result {
        Ok(Ok(resolution)) => resolution,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned source401 outcome must settle within its deadline"),
    };
    assert_eq!(
        serde_json::to_value(resolution).unwrap()["outcome"],
        json!("provider_unavailable"),
        "A provider search401 must remain unavailable instead of an accepted musical mismatch"
    );
}

async fn exercise(
    primary: &fixture::ProviderServer,
    previous_audio: &fixture::ProviderServer,
    failing: &FakePlugin,
) -> SourceResolution {
    let owned = fixture::SavedLibrary::prepare(primary, previous_audio).await;
    let connections = owned.hito.plugins.connections(&owned.actor).await.unwrap();
    let unused = connections
        .installations
        .iter()
        .find(|installation| installation.manifest.id == "org.hito.audio-fixture")
        .unwrap();
    owned
        .hito
        .plugins
        .remove(&owned.actor, &unused.id, unused.revision)
        .await
        .unwrap();
    let service = owned
        .hito
        .plugins
        .offer_service(
            &owned.actor,
            OfferPluginService {
                name: "Owned source credentials service".into(),
                base_url: failing.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = owned
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
        .collect::<Vec<_>>();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].id, installed.id);
    owned
        .hito
        .identity
        .create_user(
            &owned.actor,
            Credentials {
                username: "source-credentials-peer".into(),
                password: "owned source credentials peer password".into(),
            },
        )
        .await
        .unwrap();
    let session = owned
        .hito
        .identity
        .login(Credentials {
            username: "source-credentials-peer".into(),
            password: "owned source credentials peer password".into(),
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
    assert!(matches!(
        owned
            .hito
            .catalog
            .source_choice(&peer, &owned.recording)
            .await,
        Err(Error::NotFound)
    ));
    assert!(failing.requests_for(SEARCH_PATH).is_empty());
    let choice = serde_json::to_value(
        owned
            .hito
            .catalog
            .source_choice(&owned.actor, &owned.recording)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(choice["mode"], "automatic");
    let request_id = Uuid::new_v4();
    let resolution = owned
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
    assert_eq!(resolution.request_id, request_id);
    assert_eq!(resolution.recording_id, owned.recording);
    assert!(resolution.sources.is_empty());
    let calls = failing.requests_for(SEARCH_PATH);
    assert_eq!(calls.len(), 1);
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
        .unwrap(),
        choice
    );
    owned.assert_preserved().await;
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        assert!(previous_audio.requests(path).await.is_empty());
    }
    let serialized = serde_json::to_string(&resolution).unwrap();
    for private in [
        "primary/owned-recording",
        "audio/owned-recording",
        "Private title override",
        &primary.url,
        &previous_audio.url,
        &failing.url,
    ] {
        assert!(!serialized.contains(private));
    }
    resolution
}
