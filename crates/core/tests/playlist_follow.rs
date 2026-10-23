use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, CoreConfig, Credentials, Error, FollowProviderPlaylist, HostDeviceId,
    ImportProviderPlaylist, Operation, PlaylistFollowInterval, PlaylistFollowStatus,
    RefreshFollowedPlaylist, SyncBatch, open,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn recording(id: &str) -> Value {
    json!({"occurrence_id": null, "item": {"kind": "recording", "recording": {
        "id": id, "title": id, "artist": "Follow fixture artist"
    }}})
}

#[tokio::test]
async fn a_durable_follow_preserves_duplicate_entry_ids_and_refreshes_only_its_own_playlist() {
    let server = fixture::ProviderServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = open(CoreConfig::new(
        dir.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("playlist-follow").unwrap(),
    ))
    .await
    .unwrap();
    let creator_session = hito
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito
        .identity
        .authenticate(&creator_session.token)
        .await
        .unwrap();
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/followed",
        "snapshot-one",
        vec![
            recording("A"),
            recording("A"),
            recording("B"),
            json!({"item": {"kind": "unknown"}}),
        ],
    )
    .await;
    let request_id = Uuid::new_v4();
    let reference = "https://provider.invalid/selection";
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id,
                installation_revision: installed.revision,
                playlist_reference: reference.into(),
            },
        )
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistRename {
            playlist_id: copied.playlist_id,
            name: "My independent copy".into(),
        }],
    )
    .await;
    let copy_before = serde_json::to_value(
        hito.collection
            .playlist(&actor, &copied.playlist_id.to_string())
            .await
            .unwrap(),
    )
    .unwrap();

    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installed.id,
            FollowProviderPlaylist {
                request_id,
                installation_revision: installed.revision,
                playlist_reference: reference.into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .expect("Follow should create an independent provider-updated playlist");
    assert_eq!(followed.request_id, request_id);
    assert_ne!(followed.playlist_id, copied.playlist_id);
    let playlist = followed.playlist_id.to_string();
    let before = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(before.playlist.kind, "followed");
    assert_eq!(before.entries.len(), 4);
    assert_ne!(before.entries[0].entry_id, before.entries[1].entry_id);
    assert_eq!(
        before.entries[0].recording_id,
        before.entries[1].recording_id
    );
    let original_ids = before
        .entries
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect::<Vec<_>>();
    let details_before_replay = server.requests("v1/playlists/detail").await.len();
    let replay = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installed.id,
            FollowProviderPlaylist {
                request_id,
                installation_revision: installed.revision,
                playlist_reference: reference.into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.playlist_id, followed.playlist_id);
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        details_before_replay
    );
    hito.identity.logout(&creator_session.token).await.unwrap();
    drop(hito);

    let hito = open(CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("playlist-follow").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let retained = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(retained.interval, PlaylistFollowInterval::Hourly);
    assert_eq!(retained.snapshot, "snapshot-one");
    assert_eq!(retained.next_refresh_at - retained.last_refreshed_at, 3600);

    fixture::playlist(
        &server,
        "playlist/followed",
        "snapshot-two",
        vec![
            recording("B"),
            recording("A"),
            recording("C"),
            recording("A"),
            json!({"item": {"kind": "unknown"}}),
        ],
    )
    .await;
    let requests_before_refresh = server.requests("v1/playlists/detail").await.len();
    let refresh = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_eq!(refresh.playlist_id, followed.playlist_id);
    hito.identity.logout(&session.token).await.unwrap();
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let after = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(
        after
            .entries
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["B", "A", "C", "A", "Unknown provider item"]
    );
    assert_eq!(after.entries[0].entry_id, original_ids[2]);
    assert_eq!(after.entries[1].entry_id, original_ids[0]);
    assert_eq!(after.entries[3].entry_id, original_ids[1]);
    assert!(!original_ids.contains(&after.entries[2].entry_id));
    assert!(!original_ids.contains(&after.entries[4].entry_id));
    assert_eq!(after.entries[1].recording_id, after.entries[3].recording_id);
    let detail_requests = server.requests("v1/playlists/detail").await;
    assert_eq!(detail_requests.len(), requests_before_refresh + 2);
    assert!(
        detail_requests[requests_before_refresh..]
            .iter()
            .all(|(_, request)| request
                == &json!({"kind": "canonical", "playlist_id": "playlist/followed"}))
    );
    let current = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(current.snapshot, "snapshot-two");
    assert_eq!(current.status, PlaylistFollowStatus::Ready);
    assert!(current.error.is_none());
    assert_eq!(current.next_refresh_at - current.last_refreshed_at, 3600);
    assert!(
        !hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(
        serde_json::to_value(
            hito.collection
                .playlist(&actor, &copied.playlist_id.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        copy_before
    );
    let rejected = hito
        .collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "follow-fixture".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistRename {
                        playlist_id: followed.playlist_id,
                        name: "Cannot rename a follow".into(),
                    },
                }],
            },
        )
        .await;
    assert!(matches!(rejected, Err(Error::Conflict(_))));
    let feed = serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let publications = feed["operations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event.get("publication"))
        .collect::<Vec<_>>();
    assert_eq!(publications.len(), 3);
    assert_eq!(publications[0]["kind"], "playlist_imported");
    assert_eq!(publications[1]["kind"], "playlist_followed");
    assert_eq!(publications[2]["kind"], "playlist_follow_refreshed");
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        3
    );
    assert_eq!(server.requests("v1/sources").await.len(), 0);
    assert_eq!(server.requests("v1/audio").await.len(), 0);
}
