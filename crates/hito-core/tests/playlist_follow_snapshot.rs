#[path = "fixtures/playlist_read_barrier.rs"]
mod barrier;
#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, FollowProviderPlaylist, ImportProviderPlaylist, PlaylistFollowInterval,
    RefreshFollowedPlaylist,
};
use serde_json::json;
use std::time::Duration;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

async fn version(server: &fixture::ProviderServer, version: &str) {
    let items = ["first", "second"].into_iter().map(|position| json!({"occurrence_id": position, "item": {"kind": "recording", "recording": {
        "id": format!("recording/{version}/{position}"), "title": format!("Version {version} {position}"), "artist": "Fixture artist"
    }}})).collect::<Vec<_>>();
    fixture::playlist(
        server,
        "playlist/consistent",
        &format!("snapshot-{version}"),
        items,
    )
    .await;
    server.reply("v1/playlists/detail", json!({
        "id": "playlist/consistent", "name": format!("Version {version}"), "snapshot": format!("snapshot-{version}"),
        "total_count": 2, "attribution": {"label": "Owned snapshot fixture"}
    })).await;
}

#[tokio::test]
async fn playlist_detail_reads_one_accepted_snapshot_while_a_background_refresh_publishes() {
    let header = barrier::HeaderBarrier::default();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(header.clone()))
        .unwrap();
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) = support::library("follow-snapshot").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    version(&server, "one").await;
    let copied = hito
        .collection
        .import_provider_playlist(
            &actor,
            &installation.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/consistent-selection".into(),
            },
        )
        .await
        .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistRename {
            playlist_id: copied.playlist_id,
            name: "My independent snapshot".into(),
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
    assert!(copy_before.get("follow_content_revision").is_none());
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/consistent-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    version(&server, "two").await;
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
    let _release_on_drop = header.arm();
    let reading = {
        let collection = hito.collection.clone();
        let actor = actor.clone();
        let playlist = playlist.clone();
        tokio::spawn(async move { collection.playlist(&actor, &playlist).await })
    };
    tokio::time::timeout(Duration::from_secs(2), header.entered())
        .await
        .expect("The actual first playlist header read must reach its SQL completion barrier");
    let publication = tokio::time::timeout(
        Duration::from_secs(3),
        hito.collection
            .run_next_follow_refresh_until(std::future::pending()),
    )
    .await;
    header.release();
    let read = tokio::time::timeout(Duration::from_secs(2), reading)
        .await
        .expect("The owned playlist reader must finish after releasing its header row")
        .unwrap()
        .unwrap();
    assert!(
        !header.timed_out(),
        "The SQL completion barrier must be released by the test, not its deadline"
    );
    assert!(
        publication
            .expect("The real background refresh must commit while the old header is held")
            .unwrap()
    );
    let latest = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(latest.playlist.name, "Version two");
    assert_eq!(latest.origin.as_ref().unwrap().snapshot, "snapshot-two");
    assert_eq!(latest.follow_content_revision, Some(2));
    let (accepted, revision) = match read.origin.as_ref().unwrap().snapshot.as_str() {
        "snapshot-one" => ("one", 1),
        "snapshot-two" => ("two", 2),
        _ => panic!("Playlist origin must identify a complete accepted fixture version"),
    };
    assert_eq!(
        read.follow_content_revision,
        Some(revision),
        "A playlist detail must return its content revision from the same accepted snapshot"
    );
    assert_eq!(
        read.playlist.name,
        format!("Version {accepted}"),
        "A playlist detail must return its header and origin from the same accepted snapshot"
    );
    assert_eq!(read.entries.len(), 2);
    assert_eq!(
        read.entries
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    for (entry, position) in read.entries.iter().zip(["first", "second"]) {
        assert_eq!(
            entry
                .origin
                .as_ref()
                .unwrap()
                .provider_recording_id
                .as_deref(),
            Some(format!("recording/{accepted}/{position}").as_str())
        );
        assert_eq!(entry.title, format!("Version {accepted} {position}"));
    }
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
}
