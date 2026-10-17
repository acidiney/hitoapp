#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, CollectionPublication, FollowProviderPlaylist, ImportProviderPlaylist,
    PlaylistFollowInterval, PlaylistFollowStatus, RefreshFollowedPlaylist, SyncEvent,
};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

async fn age_refresh_time(database: &std::path::Path, playlist: &str) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let changed = sqlx::query("UPDATE playlist_follows SET last_refreshed_at = unixepoch() - 120,next_refresh_at = unixepoch() + 3480 WHERE playlist_id = ?")
        .bind(playlist).execute(&mut connection).await.unwrap().rows_affected();
    assert_eq!(changed, 1);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn changed_complete_content_with_the_same_provider_snapshot_token_publishes_the_new_snapshot()
{
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-token-reuse").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    let a = json!({"item": {"kind": "recording", "recording": {"id": "recording/A", "title": "Provider A", "artist": "Fixture artist", "revision": "one"}}});
    let b = |revision: &str, title: &str| json!({"item": {"kind": "recording", "recording": {"id": "recording/B", "title": title, "artist": "Fixture artist", "revision": revision}}});
    let unknown = || json!({"item": {"kind": "unknown"}});
    fixture::playlist(
        &server,
        "playlist/token-reuse",
        "shared-token",
        vec![a.clone(), a.clone(), b("one", "Provider B"), unknown()],
    )
    .await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installation.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/token-reuse-selection".into(),
            },
        )
        .await
        .unwrap();
    let copied_detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let recording_a =
        Uuid::parse_str(copied_detail.entries[0].recording_id.as_ref().unwrap()).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistRename {
                playlist_id: copied.playlist_id,
                name: "My token-independent copy".into(),
            },
            Action::EntryRemove {
                playlist_id: copied.playlist_id,
                entry_id: Uuid::parse_str(&copied_detail.entries[2].entry_id).unwrap(),
            },
            Action::RecordingTitleSet {
                recording_id: recording_a,
                title: "My personal A".into(),
            },
            Action::LikeSet {
                recording_id: recording_a,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recording_a,
                position_ms: 4200,
                speed: 1.25,
            },
        ],
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
                playlist_reference: "https://provider.invalid/token-reuse-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let initial = hito.collection.playlist(&actor, &playlist).await.unwrap();
    let initial_ids = initial
        .entries
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect::<Vec<_>>();
    let recording_b = initial.entries[2].recording_id.clone().unwrap();
    let copy_before = serde_json::to_value(
        hito.collection
            .playlist(&actor, &copied.playlist_id.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let metadata_before = serde_json::to_value(
        hito.catalog
            .recording_metadata(&actor, &recording_a.to_string(), Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let activity_before =
        serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap();
    let cursor = hito.collection.events(&actor, 0).await.unwrap().cursor;
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();

    fixture::playlist(&server, "playlist/token-reuse", "shared-token", vec![
        b("two", "Revised provider B"), a.clone(),
        json!({"item": {"kind": "recording", "recording": {"id": "recording/C", "title": "Provider C", "artist": "Fixture artist", "revision": "one"}}}),
        a, unknown(),
    ]).await;
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
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests + 2
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests + 1
    );
    let current = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(current.origin.as_ref().unwrap().snapshot, "shared-token");
    assert_eq!(current.entries.len(), 5);
    assert_eq!(
        current
            .entries
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Revised provider B",
            "My personal A",
            "Provider C",
            "My personal A",
            "Unknown provider item"
        ]
    );
    assert_eq!(current.entries[0].entry_id, initial_ids[2]);
    assert_eq!(current.entries[1].entry_id, initial_ids[0]);
    assert_eq!(current.entries[3].entry_id, initial_ids[1]);
    assert!(!initial_ids.contains(&current.entries[2].entry_id));
    assert_ne!(current.entries[4].entry_id, initial_ids[3]);
    assert_eq!(
        current.entries[0].recording_id.as_deref(),
        Some(recording_b.as_str())
    );
    assert_eq!(
        current.entries[1].recording_id,
        initial.entries[0].recording_id
    );
    assert_eq!(
        current.entries[3].recording_id,
        initial.entries[1].recording_id
    );
    let publication = hito.collection.events(&actor, cursor).await.unwrap();
    assert_eq!(publication.operations.len(), 1);
    assert!(publication.cursor > cursor);
    let SyncEvent::Publication {
        publication: CollectionPublication::PlaylistFollowRefreshed { snapshot, .. },
        ..
    } = &publication.operations[0]
    else {
        panic!(
            "Changed complete provider content must publish a followed-playlist snapshot even when its token is unchanged"
        );
    };
    assert_eq!(snapshot.origin.snapshot, "shared-token");
    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|entry| entry.entry_id.to_string())
            .collect::<Vec<_>>(),
        current
            .entries
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect::<Vec<_>>()
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
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .recording_metadata(&actor, &recording_a.to_string(), Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        metadata_before
    );
    assert_eq!(
        serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap(),
        activity_before
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        3
    );
    assert_eq!(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap()
            .status,
        PlaylistFollowStatus::Ready
    );
}

async fn state(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
    playlist: Uuid,
    copy: Uuid,
    recording: &str,
) -> Value {
    json!({
        "playlist": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "copy": hito.collection.playlist(actor, &copy.to_string()).await.unwrap(),
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
    })
}

#[tokio::test]
async fn refreshing_an_unchanged_complete_snapshot_updates_the_schedule_without_republishing_collection_state()
 {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library("follow-unchanged").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    let recording = json!({"item": {"kind": "recording", "recording": {"id": "recording/stable", "title": "Provider title", "artist": "Provider artist", "duration_ms": 60000, "revision": "one"}}});
    fixture::playlist(
        &server,
        "playlist/unchanged",
        "stable",
        vec![
            recording.clone(),
            recording,
            json!({"item": {"kind": "unknown"}}),
        ],
    )
    .await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installation.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/unchanged-selection".into(),
            },
        )
        .await
        .unwrap();
    let copied_detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let recording_id =
        Uuid::parse_str(copied_detail.entries[0].recording_id.as_ref().unwrap()).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistRename {
                playlist_id: copied.playlist_id,
                name: "My independent unchanged copy".into(),
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My personal title".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4200,
                speed: 1.25,
            },
        ],
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
                playlist_reference: "https://provider.invalid/unchanged-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    age_refresh_time(&directory.path().join("hito.db"), &playlist).await;
    let status_before = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    let before = state(
        &hito,
        &actor,
        followed.playlist_id,
        copied.playlist_id,
        &recording_id.to_string(),
    )
    .await;
    let cursor_before = hito.collection.events(&actor, 0).await.unwrap().cursor;
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
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
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests + 2
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests + 1
    );
    let status = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(status.status, PlaylistFollowStatus::Ready);
    assert!(status.error.is_none());
    assert_eq!(status.snapshot, "stable");
    assert_eq!(status.content_revision, status_before.content_revision);
    assert!(status.last_refreshed_at >= status_before.last_refreshed_at + 100);
    assert!(status.next_refresh_at >= status_before.next_refresh_at + 100);
    assert_eq!(status.next_refresh_at - status.last_refreshed_at, 3600);
    let after_events = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(
        after_events.cursor, cursor_before,
        "An unchanged complete provider snapshot must not publish another collection event or consume a synchronization sequence"
    );
    assert_eq!(
        state(
            &hito,
            &actor,
            followed.playlist_id,
            copied.playlist_id,
            &recording_id.to_string()
        )
        .await,
        before
    );
}
