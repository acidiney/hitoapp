#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, CollectionPublication, Error, ImportProviderPlaylist,
    ImportedPlaylistEntry, ImportedPlaylistEntryKind, ImportedPlaylistEntryOrigin,
    ImportedPlaylistOrigin, ImportedPlaylistRecording, ImportedPlaylistSnapshot,
    PluginInstallation, Services, SyncEvent,
};
use hito_plugin_sdk::{ProviderPlaylist, ProviderRecording};
use serde_json::{Value, json};
use uuid::Uuid;

const MIB: usize = 1024 * 1024;
const RECORDINGS: usize = 3_000;

#[tokio::test]
async fn an_oversized_publication_rolls_back_everything_and_allows_the_same_request_after_correction()
 {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) =
        support::library("playlist-publication-rollback").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists", "sources"]).await;
    server.reply("v1/recordings/detail", json!({
        "id": "recording/baseline", "title": "Original title", "artist": "Original artist", "duration_ms": 90_000
    })).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [
                {"id": "audio/baseline", "label": "Existing source", "ready": true}
            ]}),
        )
        .await;
    let adopted = hito
        .catalog
        .adopt_provider(
            &actor,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: "recording/baseline".into(),
            },
        )
        .await
        .unwrap();
    assert!(adopted.available);
    let recording_id = Uuid::parse_str(&adopted.id).unwrap();
    let retained = Uuid::new_v4();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: retained,
                name: "Keep existing".into(),
            },
            Action::EntryInsert {
                playlist_id: retained,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "My title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My artist".into(),
            },
        ],
    )
    .await;
    let before = public_state(&hito, &actor, &adopted.id, retained).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["catalog"]["items"][0]["liked"], true);
    assert_eq!(before["sources"][0]["ready"], true);
    assert_eq!(before["activity"]["history"][0]["position_ms"], 4_200);
    assert_eq!(before["activity"]["history"][0]["speed"], 1.25);

    let detail = json!({
        "id": "playlist/oversized", "name": "Oversized selection", "snapshot": "oversized-one",
        "total_count": RECORDINGS, "attribution": {"label": "Playlist byte fixture"}
    });
    let title = format!("{}{}", "é".repeat(700), "\"".repeat(600));
    let artist = format!("{}{}", "界".repeat(400), "\\".repeat(800));
    assert!(title.len() <= 2048 && artist.len() <= 2048);
    let items = (0..RECORDINGS).map(|index| json!({
        "occurrence_id": format!("occurrence-{index}"), "item": {"kind": "recording", "recording": {
            "id": format!("oversized/{index}"), "title": title, "artist": artist
        }}
    })).collect::<Vec<_>>();
    let mut source_bytes = serde_json::to_vec(&detail).unwrap().len() * 2;
    let mut largest_source_page = 0;
    let mut pages = Vec::new();
    for (index, chunk) in items.chunks(100).enumerate() {
        let cursor = (index != 0).then(|| format!("page-{index}"));
        let next = ((index + 1) * 100 < RECORDINGS).then(|| format!("page-{}", index + 1));
        let page = json!({
            "playlist_id": "playlist/oversized", "snapshot": "oversized-one",
            "items": chunk, "next_cursor": next
        });
        assert!(chunk.len() <= 100);
        let bytes = serde_json::to_vec(&page).unwrap().len();
        assert!(bytes <= MIB);
        source_bytes += bytes;
        largest_source_page = largest_source_page.max(bytes);
        pages.push((cursor, page));
    }
    assert_eq!(pages.len(), 30);
    assert!(source_bytes < 16 * MIB);
    let service_revision = hito
        .plugins
        .settings(&actor)
        .await
        .unwrap()
        .services
        .into_iter()
        .find(|service| service.id == installed.service_id)
        .unwrap()
        .revision;
    let minimum = publication_lower_bound(
        &installed,
        service_revision,
        Uuid::parse_str(&adopted.library_id).unwrap(),
        serde_json::from_value(detail.clone()).unwrap(),
        &items,
    );
    let publication_bytes_lower_bound = serde_json::to_vec(&minimum).unwrap().len();
    assert!(publication_bytes_lower_bound > 16 * MIB);
    let wire_bytes_lower_bound = serde_json::to_vec(&SyncEvent::Publication {
        sequence: 0,
        publication: minimum,
    })
    .unwrap()
    .len();
    assert!(wire_bytes_lower_bound > 16 * MIB);
    println!(
        "source_bytes={source_bytes} largest_source_page={largest_source_page} publication_bytes_lower_bound={publication_bytes_lower_bound} wire_bytes_lower_bound={wire_bytes_lower_bound}"
    );
    drop(items);
    server.reply("v1/playlists/detail", detail).await;
    server.playlist_pages(pages).await;
    let request_id = Uuid::new_v4();
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/oversized-selection".into(),
    };
    match hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
    {
        Err(Error::Invalid(message)) => assert_eq!(
            message,
            "The imported playlist exceeds the publication size limit"
        ),
        Err(error) => panic!("A legal provider snapshot failed before publication sizing: {error}"),
        Ok(_) => panic!("A publication over the encoded size limit must be rejected"),
    }
    assert_eq!(
        public_state(&hito, &actor, &adopted.id, retained).await,
        before
    );
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 2);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 30);

    server.playlist_pages(Vec::new()).await;
    fixture::playlist(
        &server,
        "playlist/oversized",
        "corrected-small",
        vec![
            json!({"occurrence_id": "corrected", "item": {"kind": "recording", "recording": {
                "id": "oversized/0", "title": "Corrected recording", "artist": "Fixture artist"
            }}}),
        ],
    )
    .await;
    let accepted = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    assert_eq!(accepted.request_id, request_id);
    let corrected = hito
        .collection
        .playlist(&actor, &accepted.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(corrected.entries.len(), 1);
    assert_eq!(corrected.entries[0].title, "Corrected recording");
    assert!(!corrected.entries[0].available);
    let corrected_recording = corrected.entries[0].recording_id.as_deref().unwrap();
    assert!(
        hito.catalog
            .sources(&actor, corrected_recording)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        hito.catalog
            .recording_metadata(&actor, corrected_recording, Default::default())
            .await
            .unwrap()
            .observations
            .items
            .len(),
        1
    );
    let after = public_state(&hito, &actor, &adopted.id, retained).await;
    assert_eq!(after["catalog"]["total"], 2);
    assert_eq!(after["playlists"].as_array().unwrap().len(), 2);
    for field in ["detail", "sources", "activity", "metadata"] {
        assert_eq!(after[field], before[field]);
    }
    let events = after["feed"]["operations"].as_array().unwrap();
    assert_eq!(
        events.len(),
        before["feed"]["operations"].as_array().unwrap().len() + 1
    );
    let publication = &events.last().unwrap()["publication"]["snapshot"];
    assert_eq!(publication["playlist_id"], accepted.playlist_id.to_string());
    assert_eq!(publication["origin"]["snapshot"], "corrected-small");
    assert_eq!(publication["recordings"].as_array().unwrap().len(), 1);
    assert_eq!(server.requests("v1/recordings/detail").await.len(), 1);
    assert_eq!(server.requests("v1/sources").await.len(), 1);
}

async fn public_state(hito: &Services, actor: &Actor, recording: &str, playlist: Uuid) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "detail": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap()
    })
}

fn publication_lower_bound(
    installation: &PluginInstallation,
    service_revision: i64,
    library_id: Uuid,
    detail: ProviderPlaylist,
    items: &[Value],
) -> CollectionPublication {
    let mut recordings = Vec::with_capacity(items.len());
    let mut entries = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let metadata: ProviderRecording =
            serde_json::from_value(item["item"]["recording"].clone()).unwrap();
        metadata.validate().unwrap();
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
    CollectionPublication::PlaylistImported {
        publication_id: Uuid::new_v4(),
        snapshot: ImportedPlaylistSnapshot {
            playlist_id: Uuid::new_v4(),
            name: detail.name,
            origin: ImportedPlaylistOrigin {
                installation_id: installation.id.clone(),
                installation_revision: installation.revision,
                service_id: installation.service_id.clone(),
                service_revision,
                provider: installation.manifest.id.clone(),
                provider_playlist_id: detail.id,
                snapshot: detail.snapshot,
                attribution: detail.attribution,
                imported_at: 0,
            },
            recordings,
            entries,
        },
    }
}
