use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus, PluginRefresh,
    RefreshFollowedPlaylist, RefreshPlugin,
};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn recording(occurrence: &str, recording: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": format!("recording/{recording}"), "title": format!("Recording {recording}"),
        "artist": "Owned resume fixture artist", "duration_ms": 123000
    }}})
}

#[tokio::test]
async fn an_old_worker_retires_without_overwriting_a_resumed_generations_error_schedule_or_snapshot()
 {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-resume-worker").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/retained-follow",
        "snapshot-one",
        vec![recording("first-a", "a"), recording("duplicate-a", "a")],
    )
    .await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-worker-follow".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let original = hito.collection.playlist(&actor, &playlist).await.unwrap();
    let original_json = serde_json::to_value(&original).unwrap();
    let original_catalog = serde_json::to_value(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let original_status = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(original_status.generation, 1);
    assert_eq!(original_status.status, PlaylistFollowStatus::Ready);
    fixture::playlist(
        &server,
        "playlist/retained-follow",
        "snapshot-two",
        vec![
            recording("duplicate-a", "a"),
            recording("added-b", "b"),
            recording("first-a", "a"),
        ],
    )
    .await;
    hito.collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    let provider_gate = server.hold("v1/playlists/detail").await;
    let worker = hito
        .collection
        .run_next_follow_refresh_until(std::future::pending());
    tokio::pin!(worker);
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            biased;
            _ = provider_gate.entered() => {},
            _ = &mut worker => panic!("The old public worker must reach its held provider request before it finishes"),
        }
    })
    .await
    .expect("The real old-generation worker must request the held canonical detail");
    assert_eq!(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap()
            .status,
        PlaylistFollowStatus::Running
    );

    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": "Reviewed resume fixture",
                "protocol_version": 1, "capabilities": ["playlists", "search"]
            }),
        )
        .await;
    let PluginRefresh::ReviewRequired { digest, .. } = hito
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("The changed fixture manifest must require explicit permission review");
    };
    let PluginRefresh::Updated {
        installation: approved,
    } = hito
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: Some(digest),
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("The exact reviewed fixture manifest must be accepted");
    };
    assert_eq!(approved.id, installation.id);
    assert_eq!(approved.service_id, installation.service_id);
    let paused = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(paused.status, PlaylistFollowStatus::Paused);
    let settings = hito.plugins.settings(&actor).await.unwrap();
    let service = settings
        .services
        .iter()
        .find(|service| service.id == approved.service_id)
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistFollowResume {
            playlist_id: followed.playlist_id,
            expected_generation: paused.generation,
            installation_revision: approved.revision,
            service_revision: service.revision,
        }],
    )
    .await;
    let resumed = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(resumed.generation, 2);
    assert_eq!(resumed.status, PlaylistFollowStatus::Ready);
    assert!(resumed.error.is_none());
    assert!(
        resumed.next_refresh_at
            <= SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
    );
    assert_eq!(resumed.snapshot, original_status.snapshot);
    assert_eq!(resumed.last_refreshed_at, original_status.last_refreshed_at);
    let resumed_json = serde_json::to_value(&resumed).unwrap();
    let resumed_feed =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        original_json
    );

    provider_gate.open();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut worker)
            .await
            .expect("The same retained old-generation worker future must finish after release")
            .unwrap()
    );
    let retired = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(retired.generation, 2);
    assert!(
        retired.error.is_none(),
        "Retiring an old generation must not overwrite the resumed generation's cleared error"
    );
    assert_eq!(retired.next_refresh_at, resumed.next_refresh_at);
    assert_eq!(serde_json::to_value(retired).unwrap(), resumed_json);
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        original_json
    );
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recordings(&actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        original_catalog
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        resumed_feed
    );

    let before_current_refresh = server.requests("v1/playlists/detail").await.len();
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "The resumed generation must remain due and publish without a new manual request"
    );
    let current = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(current.generation, 2);
    assert_eq!(current.status, PlaylistFollowStatus::Ready);
    assert_eq!(current.snapshot, "snapshot-two");
    assert!(current.error.is_none());
    let updated = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(updated.playlist.id, original.playlist.id);
    assert_eq!(updated.entries.len(), 3);
    assert_eq!(updated.entries[0].entry_id, original.entries[1].entry_id);
    assert_eq!(updated.entries[2].entry_id, original.entries[0].entry_id);
    assert_eq!(
        updated.entries[0].recording_id,
        original.entries[0].recording_id
    );
    assert_eq!(
        updated.entries[2].recording_id,
        original.entries[0].recording_id
    );
    assert!(
        !original
            .entries
            .iter()
            .any(|entry| entry.entry_id == updated.entries[1].entry_id)
    );
    assert_eq!(
        updated
            .entries
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let details = server.requests("v1/playlists/detail").await;
    assert_eq!(details.len(), before_current_refresh + 2);
    assert!(
        details[before_current_refresh..]
            .iter()
            .all(|(_, body)| body
                == &json!({
                    "kind": "canonical", "playlist_id": "playlist/retained-follow"
                }))
    );
    let feed = serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let events = feed["operations"].as_array().unwrap();
    let previous_events = resumed_feed["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous_events.len() + 1);
    assert_eq!(&events[..previous_events.len()], previous_events.as_slice());
    assert_eq!(
        events.last().unwrap()["publication"]["kind"],
        "playlist_follow_refreshed"
    );
    assert_eq!(server.requests("v1/sources").await.len(), 0);
    assert_eq!(server.requests("v1/audio").await.len(), 0);
}
