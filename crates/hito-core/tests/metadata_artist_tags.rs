#[path = "fixtures/musicbrainz_tags.rs"]
mod fixture;
use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;
#[path = "metadata_artist_tags/upgrade.rs"]
mod upgrade;

use hito_core::{Action, Browse, InspectedTags, MetadataHistoryQuery, Search};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn artist_identifier_tags_remain_source_assertions_without_changing_catalog_identity() {
    let (dir, hito, actor, library, music) = support::library("artist-tag-host").await;
    let input = dir.path().join("identified.wav");
    support::write_wav(&input, 0);
    let identified = music.join("identified.flac");
    fixture::encode(&input, &identified, "Identified file");
    let input = dir.path().join("namesake.wav");
    support::write_wav(&input, 1);
    let namesake = music.join("namesake.flac");
    fixture::encode(&input, &namesake, "Unidentified namesake");
    support::scan(&hito, &actor, &library).await;
    let before = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(before.total, 2);
    let tagged_id = before
        .items
        .iter()
        .find(|item| item.title == "Identified file")
        .unwrap()
        .id
        .clone();
    let namesake_id = before
        .items
        .iter()
        .find(|item| item.title == "Unidentified namesake")
        .unwrap()
        .id
        .clone();
    assert_ne!(tagged_id, namesake_id);
    let tagged_sources =
        serde_json::to_value(hito.catalog.sources(&actor, &tagged_id).await.unwrap()).unwrap();
    let namesake_sources =
        serde_json::to_value(hito.catalog.sources(&actor, &namesake_id).await.unwrap()).unwrap();
    let artists = serde_json::to_value(
        hito.catalog
            .artists(&actor, Browse::default())
            .await
            .unwrap(),
    )
    .unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: Uuid::parse_str(&tagged_id).unwrap(),
            title: "Private title pin".into(),
        }],
    )
    .await;

    fixture::add_assertions(&identified, &dir.path().join("retagged.flac"));
    let raw = fixture::probe(&identified);
    let raw_namesake = fixture::probe(&namesake);
    for (key, _, value) in fixture::ASSERTIONS {
        assert_eq!(raw["format"]["tags"][key], value);
        assert!(raw_namesake["format"]["tags"].get(key).is_none());
    }
    support::scan(&hito, &actor, &library).await;
    let detail = hito
        .catalog
        .recording_metadata(&actor, &tagged_id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let observation = detail.selected_observation.as_ref().unwrap();
    assert_eq!(observation.provider, "local");
    assert!(observation.plugin_installation_id.is_none());
    let tags = serde_json::to_value(observation.tags.as_ref().unwrap()).unwrap();
    let retained = tags
        .get("musicbrainz_artist_tags")
        .and_then(Value::as_array)
        .expect(
            "A successful inspection must retain the observed MusicBrainz artist tag assertions",
        );
    assert_eq!(retained.len(), fixture::ASSERTIONS.len());
    for (key, role, value) in fixture::ASSERTIONS {
        assert!(retained.contains(&json!({
            "key": key, "role": role, "value": value, "location": {"kind": "format"},
        })));
    }
    assert_eq!(detail.title.personal.as_deref(), Some("Private title pin"));
    assert_eq!(detail.title.inherited, "Identified file");
    assert_eq!(detail.artist.inherited, fixture::CREDIT);
    assert_eq!(detail.recording_id, tagged_id);
    assert_eq!(
        observation
            .tags
            .as_ref()
            .unwrap()
            .artist
            .as_ref()
            .unwrap()
            .value,
        fixture::CREDIT
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &tagged_id).await.unwrap()).unwrap(),
        tagged_sources
    );
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &namesake_id).await.unwrap()).unwrap(),
        namesake_sources
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
    let after = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(after.total, 2);
    assert!(
        after
            .items
            .iter()
            .any(|item| item.id == namesake_id && item.title == "Unidentified namesake")
    );
    let untouched = hito
        .catalog
        .recording_metadata(&actor, &namesake_id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let untouched =
        serde_json::to_value(untouched.selected_observation.unwrap().tags.unwrap()).unwrap();
    assert_eq!(untouched["musicbrainz_artist_tags"], json!([]));
    assert!(
        hito.plugins
            .settings(&actor)
            .await
            .unwrap()
            .installations
            .is_empty()
    );

    let old_tags: InspectedTags = serde_json::from_value(json!({
        "title": null, "artist": null, "album": null,
        "album_artist": null, "disc": null, "track": null,
    }))
    .unwrap();
    assert!(serde_json::to_value(old_tags).unwrap()["musicbrainz_artist_tags"].is_null());
}

#[tokio::test]
async fn oversized_optional_artist_tags_preserve_playback_and_private_log_values() {
    let (dir, hito, actor, library, music) = support::library("artist-tag-budget").await;
    let input = dir.path().join("owned.wav");
    support::write_wav(&input, 0);
    let path = music.join("private-tag-path-canary.flac");
    let title = "Private ordinary title canary";
    fixture::encode(&input, &path, title);
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
    let canary = "private-musicbrainz-tag-canary";
    let oversized = format!("{canary}{}", "x".repeat(70 * 1024));
    fixture::add_artist_assertion(&path, &dir.path().join("retagged.flac"), &oversized);
    let raw = fixture::probe(&path);
    assert_eq!(
        raw["format"]["tags"]["MUSICBRAINZ_ARTISTID"]
            .as_str()
            .unwrap()
            .len(),
        oversized.len()
    );
    assert!(serde_json::to_vec(&raw).unwrap().len() > 65536);
    let expected = std::fs::read(&path).unwrap();
    let logs = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(logs.subscriber()).unwrap();
    support::scan(&hito, &actor, &library).await;
    let output = logs.text();
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| {
        event["fields"]["event"] == "media_command_failed"
            && event["fields"]["reason"] == "output_limit"
    }));
    for private in [
        canary,
        "MUSICBRAINZ_ARTISTID",
        title,
        fixture::CREDIT,
        path.to_str().unwrap(),
    ] {
        assert!(
            !output.contains(private),
            "Private tag data appeared in logs"
        );
    }
    let current = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, original.id);
    assert!(
        current.items[0].available,
        "Oversized optional artist tags must not make verified audio unavailable"
    );
    let detail = hito
        .catalog
        .recording_metadata(&actor, &original.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(detail.title.inherited, title);
    assert_eq!(detail.artist.inherited, fixture::CREDIT);
    let observation = detail.selected_observation.unwrap();
    let tags = observation.tags.unwrap();
    assert_eq!(tags.title.unwrap().value, title);
    assert!(tags.musicbrainz_artist_tags.is_none());
    let sources = hito.catalog.sources(&actor, &original.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, source.id);
    assert!(sources[0].ready);
    let audio = hito.catalog.audio_file(&actor, &source.id).await.unwrap();
    assert_eq!(audio.content_type, "audio/flac");
    assert!(support::audio_bytes(audio) == expected);
}
