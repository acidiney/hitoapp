use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_core::test_support::resolution_unavailable as fixture;

use futures_util::FutureExt;
use hito_core::{
    Credentials, Error, InstallPlugin, OfferPluginService, ResolveSource, SourcePreferenceEntry,
    SourceResolution, UpdateSourcePreferences,
};
use hito_plugin_sdk::{
    Capability, DETAIL_PATH, MANIFEST_PATH, METADATA_PATH, Manifest, PROTOCOL_VERSION,
    RecordingRequest, SEARCH_PATH, SOURCES_PATH, SearchPage, SearchRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[tokio::test]
async fn a_ranked_only_detail_credential_failure_is_unavailable_after_strict_search_exhaustion() {
    let primary = fixture::ProviderServer::start().await;
    let previous_audio = fixture::ProviderServer::start().await;
    let checkpoints = Arc::new(Mutex::new(Vec::new()));
    let search_checkpoint = checkpoints.clone();
    let detail_checkpoint = checkpoints.clone();
    let empty_checkpoint = checkpoints.clone();
    let mut failing = FakePlugin::builder(Manifest {
        id: "org.hito.ranked-detail-credentials-fixture".into(),
        name: "Owned ranked detail credentials service".into(),
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
    .post(SEARCH_PATH, move |request| {
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        assert!(!request.has_authorization());
        let mut calls = search_checkpoint.lock().unwrap();
        assert!(calls.is_empty());
        calls.push("ranked_provider_search");
        let page: SearchPage = serde_json::from_value(json!({
            "items": [{"id": "ranked/owned-candidate", "title": "Other owned tune",
                "artist": "Different performer", "duration_ms": 1000}],
            "next_cursor": null,
        }))
        .unwrap();
        assert!(page.validate(20).is_ok());
        Reply::Json(serde_json::to_value(page).unwrap())
    })
    .post(DETAIL_PATH, move |request| {
        let details: RecordingRequest = serde_json::from_value(request.json()).unwrap();
        assert!(details.validate().is_ok());
        assert_eq!(details.recording_id, "ranked/owned-candidate");
        assert!(!request.has_authorization());
        let mut calls = detail_checkpoint.lock().unwrap();
        assert_eq!(
            *calls,
            vec!["ranked_provider_search", "second_strict_search"]
        );
        calls.push("ranked_only_detail");
        Reply::Status(axum::http::StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    let mut empty = FakePlugin::builder(Manifest {
        id: "org.hito.ranked-strict-checkpoint-fixture".into(),
        name: "Owned strict exhaustion checkpoint".into(),
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
    .post(SEARCH_PATH, move |request| {
        let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
        assert!(search.validate().is_ok());
        assert!(
            search.query == "Owned performer Owned tune"
                && search.cursor.is_none()
                && search.limit == 20
        );
        assert!(!request.has_authorization());
        let mut calls = empty_checkpoint.lock().unwrap();
        assert_eq!(*calls, vec!["ranked_provider_search"]);
        calls.push("second_strict_search");
        let page = SearchPage {
            items: Vec::new(),
            next_cursor: None,
        };
        assert!(page.validate(20).is_ok());
        Reply::Json(serde_json::to_value(page).unwrap())
    })
    .start()
    .await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            &primary,
            &previous_audio,
            &failing,
            &empty,
            &checkpoints,
        )))
        .catch_unwind(),
    )
    .await;
    let stopped = failing.finish().await;
    let empty_stopped = empty.finish().await;
    primary.shutdown().await;
    previous_audio.shutdown().await;
    assert!(stopped && empty_stopped);
    let resolution = match result {
        Ok(Ok(resolution)) => resolution,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned ranked-only detail401 outcome must settle within its deadline"),
    };
    assert_eq!(
        serde_json::to_value(resolution).unwrap()["outcome"],
        json!("provider_unavailable"),
        "A ranked-only detail401 must remain unavailable instead of an accepted musical mismatch"
    );
}

async fn exercise(
    primary: &fixture::ProviderServer,
    previous_audio: &fixture::ProviderServer,
    failing: &FakePlugin,
    empty: &FakePlugin,
    checkpoints: &Mutex<Vec<&'static str>>,
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
                name: "Owned ranked detail credentials service".into(),
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
    let empty_service = owned
        .hito
        .plugins
        .offer_service(
            &owned.actor,
            OfferPluginService {
                name: "Owned strict exhaustion checkpoint".into(),
                base_url: empty.url.clone(),
            },
        )
        .await
        .unwrap();
    let empty_installation = owned
        .hito
        .plugins
        .install(
            &owned.actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: empty_service.id,
                service_revision: empty_service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
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
                        installation_id: installed.id.clone(),
                    },
                    SourcePreferenceEntry::Provider {
                        installation_id: empty_installation.id.clone(),
                    },
                ],
            },
        )
        .await
        .unwrap();
    let preferences = serde_json::to_value(preferences).unwrap();
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
    assert_eq!(eligible.len(), 2);
    assert!(
        eligible
            .iter()
            .any(|installation| installation.id == installed.id)
    );
    assert!(
        eligible
            .iter()
            .any(|installation| installation.id == empty_installation.id)
    );
    owned
        .hito
        .identity
        .create_user(
            &owned.actor,
            Credentials {
                username: "ranked-detail-credentials-peer".into(),
                password: "owned ranked detail credentials peer password".into(),
            },
        )
        .await
        .unwrap();
    let session = owned
        .hito
        .identity
        .login(Credentials {
            username: "ranked-detail-credentials-peer".into(),
            password: "owned ranked detail credentials peer password".into(),
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
    let peer_preferences =
        serde_json::to_value(owned.hito.catalog.source_preferences(&peer).await.unwrap()).unwrap();
    assert!(failing.requests_for(SEARCH_PATH).is_empty());
    assert!(empty.requests_for(SEARCH_PATH).is_empty());
    assert!(checkpoints.lock().unwrap().is_empty());
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
    assert_eq!(
        *checkpoints.lock().unwrap(),
        vec![
            "ranked_provider_search",
            "second_strict_search",
            "ranked_only_detail",
        ]
    );
    assert_eq!(
        empty
            .requests()
            .iter()
            .map(|request| request.operation.as_str())
            .collect::<Vec<_>>(),
        vec![MANIFEST_PATH, SEARCH_PATH]
    );
    assert!(empty.health_requests().is_empty());
    assert_eq!(empty.unexpected(), 0);
    let calls = failing.requests_for(SEARCH_PATH);
    assert_eq!(calls.len(), 1);
    assert_eq!(failing.requests_for(DETAIL_PATH).len(), 1);
    assert_eq!(
        failing
            .requests()
            .iter()
            .map(|request| request.operation.as_str())
            .collect::<Vec<_>>(),
        vec![MANIFEST_PATH, SEARCH_PATH, DETAIL_PATH]
    );
    assert!(failing.requests_for(SOURCES_PATH).is_empty());
    assert!(failing.requests_for(METADATA_PATH).is_empty());
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
    assert_eq!(failing.requests_for(DETAIL_PATH).len(), 1);
    assert_eq!(failing.requests().len(), 3);
    assert_eq!(empty.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(empty.requests().len(), 2);
    assert_eq!(
        *checkpoints.lock().unwrap(),
        vec![
            "ranked_provider_search",
            "second_strict_search",
            "ranked_only_detail",
        ]
    );
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
        "ranked/owned-candidate",
        "Private title override",
        &primary.url,
        &previous_audio.url,
        &failing.url,
        &empty.url,
    ] {
        assert!(!serialized.contains(private));
    }
    resolution
}
