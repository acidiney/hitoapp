#[path = "fixtures/musicbrainz_tags.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "fixtures/musicbrainz_large_tag.rs"]
mod large;
use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

use hito_core::{Action, MetadataHistoryQuery, Search};
use serde_json::Value;
use uuid::Uuid;

const RAW_BYTES: usize = 1024 * 1024;
const ORDINARY_TITLE: &str = "Private megabyte ordinary title canary";
const PRIVATE_TITLE: &str = "Private megabyte title pin canary";
const PRIVATE_PLAYLIST: &str = "Private megabyte playlist canary";
const TAG_CANARY: &str = "private-megabyte-artist-id-canary";

#[tokio::test]
async fn a_megabyte_optional_artist_tag_preserves_playback_and_private_collection() {
    let (directory, hito, actor, library, music) = support::library("large-artist-tag").await;
    let input = directory.path().join("owned.wav");
    support::write_wav(&input, 0);
    let path = music.join("private-megabyte-path-canary.flac");
    fixture::encode(&input, &path, ORDINARY_TITLE);
    support::scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = hito
        .catalog
        .sources(&actor, &original.id)
        .await
        .unwrap()
        .remove(0);
    let recording = Uuid::parse_str(&original.id).unwrap();
    let playlist = Uuid::new_v4();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: recording,
                title: PRIVATE_TITLE.into(),
            },
            Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: PRIVATE_PLAYLIST.into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: recording,
                position: 0,
            },
        ],
    )
    .await;
    let before = hito
        .catalog
        .recording_metadata(&actor, &original.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let title = serde_json::to_value(before.title).unwrap();
    let old_observation = before.selected_observation.unwrap();
    let old_observation_id = old_observation.id.clone();
    let old_observation = serde_json::to_value(old_observation).unwrap();
    let private_playlist = serde_json::to_value(
        hito.collection
            .playlist(&actor, &playlist.to_string())
            .await
            .unwrap(),
    )
    .unwrap();
    let operations =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let sources =
        serde_json::to_value(hito.catalog.sources(&actor, &original.id).await.unwrap()).unwrap();

    let raw = format!("{TAG_CANARY}{}", "x".repeat(RAW_BYTES - TAG_CANARY.len()));
    large::add_artist_assertion(
        &path,
        directory.path(),
        &[
            ("title", ORDINARY_TITLE),
            ("artist", fixture::CREDIT),
            ("album", "Owned album"),
            ("album_artist", fixture::ALBUM_CREDIT),
        ],
        &raw,
    );
    let expanded = large::probe(&path, true);
    assert_eq!(
        expanded.document["format"]["tags"]["MUSICBRAINZ_ARTISTID"]
            .as_str()
            .unwrap()
            .len(),
        RAW_BYTES
    );
    assert!(expanded.output_bytes > 65536);
    let ordinary = large::probe(&path, false);
    assert!(ordinary.output_bytes <= 65536);
    assert!(
        ordinary.document["format"]["tags"]
            .get("MUSICBRAINZ_ARTISTID")
            .is_none()
    );
    assert_eq!(ordinary.document["format"]["tags"]["title"], ORDINARY_TITLE);
    assert_eq!(
        ordinary.document["format"]["tags"]["artist"],
        fixture::CREDIT
    );
    let expected = std::fs::read(&path).unwrap();
    let logs = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(logs.subscriber()).unwrap();
    support::scan(&hito, &actor, &library).await;
    hito.storage.finish_indexing().await.unwrap();
    let output = logs.text();
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events.iter().any(|event| event["target"]
            .as_str()
            .is_some_and(|target| target.starts_with("hito_core::library_storage"))),
        "The scan must produce active captured core logs"
    );
    for private in [
        TAG_CANARY,
        "MUSICBRAINZ_ARTISTID",
        ORDINARY_TITLE,
        PRIVATE_TITLE,
        PRIVATE_PLAYLIST,
        fixture::CREDIT,
        directory.path().to_str().unwrap(),
    ] {
        assert!(
            !output.contains(private),
            "Private metadata appeared in scan logs"
        );
    }

    let current = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, original.id);
    assert_eq!(current.items[0].title, PRIVATE_TITLE);
    assert!(current.items[0].liked);
    assert!(
        current.items[0].available,
        "A megabyte optional artist tag must not make verified audio unavailable"
    );
    let detail = hito
        .catalog
        .recording_metadata(&actor, &original.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(detail.title).unwrap(), title);
    assert_eq!(detail.artist.inherited, fixture::CREDIT);
    let selected = detail.selected_observation.unwrap();
    assert_ne!(selected.id, old_observation_id);
    let tags = selected.tags.unwrap();
    assert_eq!(tags.title.unwrap().value, ORDINARY_TITLE);
    assert_eq!(tags.artist.unwrap().value, fixture::CREDIT);
    assert_eq!(tags.album.unwrap().value, "Owned album");
    assert_eq!(tags.album_artist.unwrap().value, fixture::ALBUM_CREDIT);
    assert!(tags.musicbrainz_artist_tags.is_none());
    let retained = detail
        .observations
        .items
        .iter()
        .find(|observation| observation.id == old_observation_id)
        .unwrap();
    assert_eq!(serde_json::to_value(retained).unwrap(), old_observation);
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &original.id).await.unwrap()).unwrap(),
        sources
    );
    assert_eq!(
        serde_json::to_value(
            hito.collection
                .playlist(&actor, &playlist.to_string())
                .await
                .unwrap()
        )
        .unwrap(),
        private_playlist
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        operations
    );
    let audio = hito.catalog.audio_file(&actor, &source.id).await.unwrap();
    assert_eq!(audio.content_type, "audio/flac");
    assert!(support::audio_bytes(audio) == expected);
    assert!(std::fs::read(&path).unwrap() == expected);
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["event"] == "artist_tag_probe_fallback")
    );
}
