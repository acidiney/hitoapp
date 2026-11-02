use hito_core::test_support::content_service as fixture;
use hito_core::test_support::hang_guard::hang_guard;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{Action, Actor, Error, InstallPlugin, OfferPluginService, Services};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

const TOKEN: &str = "private-install-review-token-sentinel";
const NAME: &str = "private-install-review-provider-sentinel";

#[tokio::test]
async fn a_declared_policy_cannot_install_or_save_a_credential_without_reviewed_acceptance() {
    let (_directory, hito, actor, library, music) = hang_guard(
        "the temporary library must open",
        support::library("plugin-install-review"),
    )
    .await;
    let original_path = music.join("owned-original.wav");
    let original = support::write_wav(&original_path, 613);
    let server = fixture::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        AssertUnwindSafe(exercise(&hito, &actor, &library, &server)).catch_unwind(),
    )
    .await;
    let settled =
        tokio::time::timeout(Duration::from_secs(5), hito.downloads.finish_downloads()).await;
    let stopped = tokio::time::timeout(Duration::from_secs(2), server.shutdown()).await;
    assert!(
        matches!(settled, Ok(Ok(()))),
        "managed filesystem work must settle on success or panic"
    );
    assert!(stopped.is_ok(), "the owned provider must stop and join");
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "installation review must preserve the owned original audio"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public installation review workflow exceeded its deadline"),
    }
}

async fn exercise(hito: &Services, actor: &Actor, library: &str, server: &fixture::ProviderServer) {
    support::scan(hito, actor, library).await;
    let recording = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    let playlist_id = Uuid::new_v4();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::PlaylistCreate {
                playlist_id,
                name: "private-install-review-playlist-sentinel".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "private-install-review-title-sentinel".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 600,
                speed: 1.25,
            },
        ],
    )
    .await;
    let playlist = playlist_id.to_string();
    let preserved = private_state(hito, actor, &recording.id, &playlist).await;
    let mut manifest = json!({
        "id": "org.hito.install-review-fixture",
        "name": NAME,
        "protocol_version": 1,
        "capabilities": ["search", "sources"],
        "content_policy": {"source_persistence": "explicit_download"}
    });
    server.reply("v1/manifest", manifest.clone()).await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned installation review fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let request = || {
        serde_json::from_value::<InstallPlugin>(json!({
            "service_id": service.id,
            "service_revision": service.revision,
            "bearer_token": TOKEN
        }))
        .unwrap()
    };
    let result = hito.plugins.install(actor, request()).await;
    let settings = hito.plugins.settings(actor).await.unwrap();
    assert!(
        settings.installations.is_empty(),
        "a declared policy without reviewed acceptance must not persist an installation or credential"
    );
    let message = match result {
        Err(Error::Conflict(message)) => message,
        Err(error) => panic!("expected a review conflict, received {}", error.code()),
        Ok(_) => panic!("a declared policy must require explicit reviewed acceptance"),
    };
    assert!(message.len() <= 256 && message.to_ascii_lowercase().contains("review"));
    assert!(!message.chars().any(char::is_control));
    for private in [TOKEN, NAME, &server.url, "private-install-review"] {
        assert!(
            !message.contains(private),
            "review errors must omit private inputs"
        );
    }
    assert!(
        private_state(hito, actor, &recording.id, &playlist).await == preserved,
        "an unaccepted declaration must preserve catalog and personal musical state"
    );
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());

    manifest.as_object_mut().unwrap().remove("content_policy");
    server.reply("v1/manifest", manifest).await;
    let legacy = hito.plugins.install(actor, request()).await.unwrap();
    assert!(legacy.manifest.content_policy.is_none() && legacy.credential_set);
    hito.plugins
        .remove(actor, &legacy.id, legacy.revision)
        .await
        .unwrap();
    assert!(
        hito.plugins
            .settings(actor)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    assert!(
        private_state(hito, actor, &recording.id, &playlist).await == preserved,
        "legacy connection and removal must preserve existing musical state"
    );
    let requests = server.requests("v1/manifest").await;
    assert_eq!(requests.len(), 2);
    let expected_bearer = format!("Bearer {TOKEN}");
    for (headers, _) in requests {
        assert!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                == Some(expected_bearer.as_str())
        );
        assert!(headers.get("cookie").is_none());
        assert_eq!(headers["x-hito-plugin-protocol"], "1");
    }
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/audio",
    ] {
        assert!(
            server.requests(path).await.is_empty(),
            "installation must request only a manifest"
        );
    }
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
}

async fn private_state(hito: &Services, actor: &Actor, recording: &str, playlist: &str) -> Value {
    json!({
        "libraries": hito.catalog.libraries(actor).await.unwrap(),
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "playlist": hito.collection.playlist(actor, playlist).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap()
    })
}
