#![cfg(unix)]

use hito_core::test_support::media_tool;
#[path = "../fixtures/metadata_tags_v2.rs"]
mod prior;

use super::{fixture, support};
use hito_core::{Action, Browse, CoreConfig, HostDeviceId, MetadataHistoryQuery, Search};
use uuid::Uuid;

#[tokio::test]
async fn unchanged_version_two_inspections_refresh_tags_without_decoding_audio() {
    let (dir, initial, actor, library, music) = support::library("artist-tag-upgrade").await;
    let input = dir.path().join("owned.wav");
    support::write_wav(&input, 0);
    let path = music.join("unchanged.flac");
    fixture::encode(&input, &path, "Unchanged tagged recording");
    fixture::add_assertions(&path, &dir.path().join("retagged.flac"));
    fixture::add_isrc(&path, &dir.path().join("isrc.flac"), "USRC17607839");
    let original = std::fs::read(&path).unwrap();
    support::scan(&initial, &actor, &library).await;
    let recording = initial
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let sources = initial
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap();
    let source = sources[0].id.clone();
    let sources = serde_json::to_value(sources).unwrap();
    let artists = serde_json::to_value(
        initial
            .catalog
            .artists(&actor, Browse::default())
            .await
            .unwrap(),
    )
    .unwrap();
    support::synchronize(
        &initial,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: Uuid::parse_str(&recording.id).unwrap(),
            title: "Retained private title".into(),
        }],
    )
    .await;
    drop(initial);
    let database = dir.path().join("hito.db");
    prior::represent_prior_inspection(&database, &source).await;

    let probes = dir.path().join("metadata-probes");
    let decodes = dir.path().join("audio-decodes");
    let mut config = CoreConfig::new(
        database,
        music,
        HostDeviceId::new("artist-tag-upgrade").unwrap(),
    );
    config.media_tools.ffprobe = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "printf 'probe\\n' >> {}\nexec /usr/bin/ffprobe \"$@\"",
            media_tool::quote(&probes),
        ),
    );
    config.media_tools.ffmpeg = media_tool::script(
        dir.path(),
        "ffmpeg",
        &format!(
            "printf 'decode\\n' >> {}\nexit 91",
            media_tool::quote(&decodes),
        ),
    );
    let hito = hito_core::open(config).await.unwrap();
    let previous = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(previous.observations.items.len(), 1);
    let previous_observation = previous.selected_observation.unwrap();
    assert_eq!(previous_observation.inspection_version, Some(2));
    assert!(
        previous_observation
            .tags
            .as_ref()
            .unwrap()
            .musicbrainz_artist_tags
            .is_none()
    );
    let previous_id = previous_observation.id.clone();
    let previous_observation = serde_json::to_value(previous_observation).unwrap();
    let private_title = serde_json::to_value(previous.title).unwrap();
    assert!(private_title["operation_id"].is_string());

    support::scan(&hito, &actor, &library).await;
    assert_eq!(std::fs::read_to_string(&probes).unwrap().lines().count(), 1);
    assert!(
        !decodes.exists(),
        "Metadata refresh must reuse the audio proof"
    );
    let current = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(current.recording_id, recording.id);
    assert_eq!(serde_json::to_value(&current.title).unwrap(), private_title);
    assert_eq!(current.observations.items.len(), 2);
    let selected = current.selected_observation.unwrap();
    assert_ne!(selected.id, previous_id);
    assert_eq!(selected.inspection_version, Some(4));
    assert_eq!(
        selected.tags.as_ref().unwrap().isrc.as_ref().unwrap().value,
        "USRC17607839"
    );
    assert_eq!(
        selected
            .tags
            .as_ref()
            .unwrap()
            .musicbrainz_artist_tags
            .as_ref()
            .unwrap()
            .len(),
        fixture::ASSERTIONS.len()
    );
    let retained = current
        .observations
        .items
        .iter()
        .find(|observation| observation.id == previous_id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(retained).unwrap(),
        previous_observation
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &recording.id).await.unwrap()).unwrap(),
        sources
    );
    assert_eq!(
        serde_json::to_value(
            hito.catalog
                .artists(&actor, Browse::default())
                .await
                .unwrap()
        )
        .unwrap(),
        artists
    );
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].id, recording.id);
    assert!(recordings.items[0].available);
    assert!(
        support::audio_bytes(hito.playback.audio_file(&actor, &source).await.unwrap()) == original
    );
    assert!(std::fs::read(path).unwrap() == original);

    support::scan(&hito, &actor, &library).await;
    let repeated = hito
        .catalog
        .recording_metadata(&actor, &recording.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(repeated.selected_observation.unwrap().id, selected.id);
    assert_eq!(repeated.observations.items.len(), 2);
    assert_eq!(std::fs::read_to_string(probes).unwrap().lines().count(), 1);
    assert!(!decodes.exists());
}
