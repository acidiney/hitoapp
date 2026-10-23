use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use hito_core::{
    Actor, AdoptProviderRecording, AttachReviewedProviderSource, Credentials, InstallPlugin,
    OfferPluginService, OpenedAudio, PluginInstallation, ProviderCandidate, Services,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn logout_stops_multi_binding_refresh_before_another_provider_request() {
    let first = fixture::ProviderServer::start().await;
    let second = fixture::ProviderServer::start().await;
    let (_directory, hito, owner, _library, _music) = support::library("binding-authority").await;
    for (server, name) in [(&first, "first"), (&second, "second")] {
        server.reply("v1/recordings/detail", json!({
            "id": format!("{name}/item"), "title": format!("{name} track"), "artist": "Owned fixture"
        })).await;
        server
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": format!("{name}/audio"), "label": "Owned source", "ready": true
                }]}),
            )
            .await;
    }
    let first_installation = connect(&hito, &owner, &first).await;
    let second_installation = connect(&hito, &owner, &second).await;
    let recording = hito
        .catalog
        .adopt_provider(
            &owner,
            &first_installation.id,
            AdoptProviderRecording {
                installation_revision: first_installation.revision,
                provider_recording_id: "first/item".into(),
            },
        )
        .await
        .unwrap();
    let candidate = ProviderCandidate {
        installation_id: second_installation.id,
        installation_revision: second_installation.revision,
        provider_recording_id: "second/item".into(),
    };
    let review = hito
        .catalog
        .review_provider_source(&owner, &recording.id, candidate.clone())
        .await
        .unwrap();
    hito.catalog
        .attach_provider_source(
            &owner,
            &recording.id,
            AttachReviewedProviderSource {
                candidate,
                review_stamp: review.review_stamp,
            },
        )
        .await
        .unwrap();
    let before =
        serde_json::to_value(hito.catalog.sources(&owner, &recording.id).await.unwrap()).unwrap();
    assert_eq!(before.as_array().unwrap().len(), 2);
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let count =
        first.requests("v1/sources").await.len() + second.requests("v1/sources").await.len();
    let first_gate = first.hold("v1/sources").await;
    let second_gate = second.hold("v1/sources").await;
    let pending = {
        let catalog = hito.catalog.clone();
        let recording = recording.id.clone();
        tokio::spawn(async move { catalog.refresh_sources(&actor, &recording).await })
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            _ = first_gate.entered() => {},
            _ = second_gate.entered() => {},
        }
    })
    .await
    .expect("one owned provider must receive the first refresh request");
    hito.identity.logout(&session.token).await.unwrap();
    first_gate.open();
    second_gate.open();
    let result = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .expect("logout must finish the refresh promptly")
        .unwrap();
    assert!(matches!(result, Err(hito_core::Error::Unauthorized)));
    assert_eq!(
        first.requests("v1/sources").await.len() + second.requests("v1/sources").await.len(),
        count + 1,
        "a logged-out actor must not start the second provider request"
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&owner, &recording.id).await.unwrap()).unwrap(),
        before,
        "logout must preserve both previously accepted source bindings"
    );
}

#[tokio::test]
async fn distinct_connections_keep_equal_remote_source_ids_and_audio_authority_separate() {
    let first = fixture::ProviderServer::start().await;
    let second = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _library, _music) =
        support::library("binding-source-identity").await;
    let first_bytes = support::write_wav(&directory.path().join("first.wav"), 10);
    let second_bytes = support::write_wav(&directory.path().join("second.wav"), 20);
    assert_ne!(first_bytes, second_bytes);
    for (server, bytes, id, title) in [
        (&first, &first_bytes, "first/item", "Primary title"),
        (&second, &second_bytes, "second/item", "Candidate title"),
    ] {
        server
            .reply(
                "v1/recordings/detail",
                json!({
                    "id": id, "title": title, "artist": "Owned artist", "duration_ms": 123_000
                }),
            )
            .await;
        server
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": "audio", "label": "Owned source", "ready": true
                }]}),
            )
            .await;
        server
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([(
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                )]),
                bytes.clone(),
            )
            .await;
    }
    let first_installation = connect(&hito, &actor, &first).await;
    let second_installation = connect(&hito, &actor, &second).await;
    let recording = hito
        .catalog
        .adopt_provider(
            &actor,
            &first_installation.id,
            AdoptProviderRecording {
                installation_revision: first_installation.revision,
                provider_recording_id: "first/item".into(),
            },
        )
        .await
        .unwrap();
    let first_source = hito
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    let before = hito
        .catalog
        .recording_metadata(&actor, &recording.id, Default::default())
        .await
        .unwrap();
    let candidate = ProviderCandidate {
        installation_id: second_installation.id,
        installation_revision: second_installation.revision,
        provider_recording_id: "second/item".into(),
    };
    let review = hito
        .catalog
        .review_provider_source(&actor, &recording.id, candidate.clone())
        .await
        .unwrap();
    let attached = hito
        .catalog
        .attach_provider_source(
            &actor,
            &recording.id,
            AttachReviewedProviderSource {
                candidate,
                review_stamp: review.review_stamp,
            },
        )
        .await
        .expect("separate provider connections may use the same remote source identity");
    assert_eq!(attached.id, recording.id);
    assert_eq!(attached.inherited_title, "Primary title");
    assert_eq!(attached.inherited_artist, "Owned artist");
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    let after = hito
        .catalog
        .recording_metadata(&actor, &recording.id, Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(after.selected_observation).unwrap(),
        serde_json::to_value(before.selected_observation).unwrap()
    );
    let sources = hito.catalog.sources(&actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 2);
    assert_ne!(sources[0].id, sources[1].id);
    assert!(
        sources
            .iter()
            .all(|source| source.recording_id == recording.id && source.ready)
    );
    assert!(sources.iter().any(|source| source.id == first_source));
    for source in sources {
        let grant = hito
            .catalog
            .playback_source(&actor, &recording.id, &source.id)
            .await
            .unwrap();
        assert!(!grant.is_local());
        let OpenedAudio::Content(mut audio) = grant.open(Default::default()).await.unwrap() else {
            panic!("each attached source must return its owned audio");
        };
        let mut body = audio.body.take().unwrap();
        let mut received = Vec::new();
        while let Some(chunk) = body.chunk().await.unwrap() {
            received.extend_from_slice(&chunk);
        }
        let expected = if source.id == first_source {
            &first_bytes
        } else {
            &second_bytes
        };
        assert_eq!(received.as_slice(), expected.as_slice());
    }
    for server in [&first, &second] {
        let requests = server.requests("v1/audio").await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].1["source_id"], "audio");
    }
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned authority fixture".into(),
                base_url: server.url.clone(),
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
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}
