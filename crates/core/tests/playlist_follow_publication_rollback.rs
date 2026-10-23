use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, CollectionPublication, FollowProviderPlaylist, ImportedPlaylistEntry,
    ImportedPlaylistEntryKind, ImportedPlaylistEntryOrigin, ImportedPlaylistOrigin,
    ImportedPlaylistRecording, ImportedPlaylistSnapshot, PlaylistFollowInterval,
    PlaylistFollowStatus, RefreshFollowedPlaylist, Services, SyncEvent,
};
use hito_plugin_sdk::{PlaylistPage, ProviderPlaylist, ProviderRecording};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

const MIB: usize = 1024 * 1024;
const RECORDINGS: usize = 3_000;
const PLAYLIST: &str = "playlist/follow-rollback";

#[tokio::test]
async fn a_rejected_follow_publication_preserves_its_revision_and_allows_a_corrected_refresh() {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) =
        support::library("follow-publication-rollback").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        PLAYLIST,
        "initial",
        vec![retained("first"), retained("duplicate")],
    )
    .await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installed.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/follow-rollback-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let initial = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(initial.entries.len(), 2);
    assert_ne!(initial.entries[0].entry_id, initial.entries[1].entry_id);
    assert_eq!(
        initial.entries[0].recording_id,
        initial.entries[1].recording_id
    );
    let recording = initial.entries[0].recording_id.as_deref().unwrap();
    let recording_id = Uuid::parse_str(recording).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My retained title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My retained artist".into(),
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
    let before = public_state(&hito, &actor, &playlist, recording).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(
        before["metadata"]["observations"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(before["feed"]["has_more"], false);
    let ready = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(ready.status, PlaylistFollowStatus::Ready);
    assert_eq!(ready.content_revision, 1);
    let details_before = server.requests("v1/playlists/detail").await.len();
    let pages_before = server.requests("v1/playlists/items").await.len();

    let oversized = ProviderPlaylist {
        id: PLAYLIST.into(),
        name: "Selection too large to publish".into(),
        snapshot: "oversized".into(),
        total_count: RECORDINGS as u32,
        attribution: hito_plugin_sdk::PlaylistAttribution {
            label: "Owned byte fixture".into(),
        },
    };
    oversized.validate().unwrap();
    let title = format!("{}{}", "é".repeat(700), "\"".repeat(600));
    let artist = format!("{}{}", "界".repeat(400), "\\".repeat(800));
    assert!(title.len() <= 2048 && artist.len() <= 2048);
    let items = (0..RECORDINGS).map(|index| json!({
        "occurrence_id": if index == 0 { "first".into() } else { format!("oversized-{index}") },
        "item": {"kind": "recording", "recording": {
            "id": if index == 0 { "recording/retained".into() } else { format!("oversized/{index}") },
            "title": title, "artist": artist
        }}
    })).collect::<Vec<_>>();
    let mut source_bytes = serde_json::to_vec(&oversized).unwrap().len() * 2;
    let mut largest_page = 0;
    let mut pages = Vec::new();
    for (index, chunk) in items.chunks(100).enumerate() {
        let cursor = (index != 0).then(|| format!("page-{index}"));
        let next = ((index + 1) * 100 < RECORDINGS).then(|| format!("page-{}", index + 1));
        let page = json!({
            "playlist_id": PLAYLIST, "snapshot": oversized.snapshot,
            "items": chunk, "next_cursor": next
        });
        serde_json::from_value::<PlaylistPage>(page.clone())
            .unwrap()
            .validate(100)
            .unwrap();
        let bytes = serde_json::to_vec(&page).unwrap().len();
        assert!(bytes <= MIB);
        source_bytes += bytes;
        largest_page = largest_page.max(bytes);
        pages.push((cursor, page));
    }
    assert_eq!(pages.len(), 30);
    assert!(source_bytes < 16 * MIB);
    let origin = serde_json::from_value(before["detail"]["origin"].clone()).unwrap();
    let library_id = Uuid::parse_str(
        before["catalog"]["items"][0]["library_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let minimum =
        publication_lower_bound(followed.playlist_id, library_id, origin, &oversized, &items);
    let publication_bytes = serde_json::to_vec(&minimum).unwrap().len();
    assert!(publication_bytes > 16 * MIB - 512);
    let wire_bytes = serde_json::to_vec(&SyncEvent::Publication {
        sequence: 0,
        publication: minimum,
    })
    .unwrap()
    .len();
    assert!(wire_bytes > 16 * MIB);
    println!(
        "source_bytes={source_bytes} largest_page={largest_page} publication_bytes_lower_bound={publication_bytes} wire_bytes_lower_bound={wire_bytes}"
    );
    drop(items);

    server
        .reply(
            "v1/playlists/detail",
            serde_json::to_value(oversized).unwrap(),
        )
        .await;
    server.playlist_pages(pages).await;
    let rejected = hito
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
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let failed = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(failed.status, PlaylistFollowStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("refresh_failed"));
    assert_eq!(failed.content_revision, 1);
    assert_eq!(failed.snapshot, "initial");
    assert_eq!(failed.last_refreshed_at, ready.last_refreshed_at);
    assert!(
        public_state(&hito, &actor, &playlist, recording).await == before,
        "A failed encoded publication must preserve the entire accepted collection state"
    );
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        details_before + 2
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        pages_before + 30
    );

    server.playlist_pages(Vec::new()).await;
    fixture::playlist(&server, PLAYLIST, "corrected", vec![
        retained("duplicate"),
        json!({"occurrence_id": "corrected", "item": {"kind": "recording", "recording": {
            "id": "oversized/1", "title": "Corrected recording", "artist": "Owned fixture artist"
        }}}),
        retained("first"),
    ]).await;
    let corrected = hito
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
    assert_ne!(corrected.request_id, rejected.request_id);
    assert_ne!(corrected.job_id, rejected.job_id);
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let accepted = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(accepted.status, PlaylistFollowStatus::Ready);
    assert_eq!(accepted.content_revision, 2);
    assert_eq!(accepted.snapshot, "corrected");
    assert!(accepted.error.is_none());
    let detail = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(detail.entries.len(), 3);
    assert_eq!(detail.entries[0].entry_id, initial.entries[1].entry_id);
    assert_eq!(detail.entries[2].entry_id, initial.entries[0].entry_id);
    assert_eq!(
        detail.entries[0].recording_id,
        initial.entries[1].recording_id
    );
    assert_eq!(
        detail.entries[2].recording_id,
        initial.entries[0].recording_id
    );
    assert!(
        initial
            .entries
            .iter()
            .all(|entry| entry.entry_id != detail.entries[1].entry_id)
    );
    assert_eq!(detail.entries[1].title, "Corrected recording");
    assert!(detail.entries.iter().all(|entry| !entry.available));
    let added = detail.entries[1].recording_id.as_deref().unwrap();
    assert!(
        hito.catalog
            .sources(&actor, added)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        hito.catalog
            .recording_metadata(&actor, added, Default::default())
            .await
            .unwrap()
            .observations
            .items
            .len(),
        1
    );
    let after = public_state(&hito, &actor, &playlist, recording).await;
    assert_eq!(after["catalog"]["total"], 2);
    assert_eq!(after["playlists"].as_array().unwrap().len(), 1);
    for field in ["metadata", "sources", "activity"] {
        assert!(
            after[field] == before[field],
            "Saved private state must survive a corrected publication"
        );
    }
    assert_eq!(after["feed"]["has_more"], false);
    let events = after["feed"]["operations"].as_array().unwrap();
    let previous = before["feed"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous.len() + 1);
    assert!(
        events[..previous.len()] == previous[..],
        "Only the accepted refresh may append a publication"
    );
    let publication = &events.last().unwrap()["publication"];
    assert_eq!(publication["kind"], "playlist_follow_refreshed");
    assert_eq!(publication["snapshot"]["playlist_id"], playlist);
    assert_eq!(publication["snapshot"]["origin"]["snapshot"], "corrected");
    assert_eq!(
        publication["snapshot"]["recordings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        publication["snapshot"]["entries"].as_array().unwrap().len(),
        3
    );
    let requests = server.requests("v1/playlists/detail").await;
    assert_eq!(requests.len(), details_before + 4);
    assert!(requests[details_before..].iter().all(|(_, request)| request
        == &json!({
            "kind": "canonical", "playlist_id": PLAYLIST
        })));
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        pages_before + 31
    );
    for path in ["v1/recordings/detail", "v1/sources", "v1/audio"] {
        assert!(server.requests(path).await.is_empty());
    }
}

fn retained(occurrence: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": "recording/retained", "title": "Original recording", "artist": "Original artist"
    }}})
}

async fn public_state(hito: &Services, actor: &Actor, playlist: &str, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "detail": hito.collection.playlist(actor, playlist).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap(),
    })
}

fn publication_lower_bound(
    playlist_id: Uuid,
    library_id: Uuid,
    mut origin: ImportedPlaylistOrigin,
    detail: &ProviderPlaylist,
    items: &[Value],
) -> CollectionPublication {
    let mut recordings = Vec::with_capacity(items.len());
    let mut entries = Vec::with_capacity(items.len());
    let mut distinct = HashSet::new();
    for (position, item) in items.iter().enumerate() {
        let metadata: ProviderRecording =
            serde_json::from_value(item["item"]["recording"].clone()).unwrap();
        metadata.validate().unwrap();
        assert!(distinct.insert(metadata.id.clone()));
        let recording_id = Uuid::new_v4();
        entries.push(ImportedPlaylistEntry {
            entry_id: Uuid::new_v4(),
            recording_id: Some(recording_id),
            position: position as u32,
            origin: ImportedPlaylistEntryOrigin {
                kind: ImportedPlaylistEntryKind::Recording,
                provider_recording_id: Some(metadata.id.clone()),
                occurrence_id: Some(item["occurrence_id"].as_str().unwrap().into()),
            },
            title: None,
            artist: None,
        });
        recordings.push(ImportedPlaylistRecording {
            recording_id,
            library_id,
            release_id: None,
            observation_id: Uuid::new_v4(),
            metadata,
        });
    }
    origin.snapshot = detail.snapshot.clone();
    origin.attribution = detail.attribution.clone();
    origin.imported_at = 0;
    CollectionPublication::PlaylistFollowRefreshed {
        publication_id: Uuid::new_v4(),
        interval: PlaylistFollowInterval::Hourly,
        snapshot: ImportedPlaylistSnapshot {
            playlist_id,
            name: detail.name.clone(),
            origin,
            recordings,
            entries,
        },
    }
}
