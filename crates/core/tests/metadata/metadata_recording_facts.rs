use hito_core::test_support::support;
use hito_core::{Action, Credentials, MetadataHistoryQuery, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::process::Command;
use uuid::Uuid;

fn encode(source: &std::path::Path, target: &std::path::Path, encoder: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(source)
        .args(encoder)
        .arg(target)
        .status()
        .unwrap();
    assert!(status.success());
}

async fn only_recording_detail(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
) -> hito_core::RecordingMetadata {
    let page = hito
        .catalog
        .recordings(actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    hito.catalog
        .recording_metadata(actor, &page.items[0].id, MetadataHistoryQuery::default())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_scanned_lossless_file_reports_its_format_size_and_release_facts() {
    let (dir, hito, actor, library, music) = support::library("facts-host").await;
    let source = dir.path().join("source.wav");
    support::write_wav(&source, 0);
    let path = music.join("tagged.flac");
    encode(&source, &path, &["-c:a", "flac", "-sample_fmt", "s16"]);
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title("Facts".into());
    tag.set_artist("Facts artist".into());
    tag.set_track(3);
    tag.set_track_total(11);
    assert!(tag.insert_text(ItemKey::RecordingDate, "2021-05-04".into()));
    assert!(tag.insert_text(ItemKey::Label, "Fixture Records".into()));
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    let detail = only_recording_detail(&hito, &actor).await;
    let format = detail.audio_format.expect("the ready file has a format");
    assert_eq!(format.format.as_deref(), Some("FLAC"));
    assert_eq!(format.bit_depth, Some(16));
    assert_eq!(format.sample_rate_hz, Some(8000));
    assert_eq!(
        format.size_bytes,
        std::fs::metadata(&path).unwrap().len() as i64
    );
    assert_eq!(detail.release_facts.year, Some(2021));
    assert_eq!(detail.release_facts.track_total, Some(11));
    assert_eq!(
        detail.release_facts.label.as_deref(),
        Some("Fixture Records")
    );
}

#[tokio::test]
async fn a_lossy_file_without_those_tags_omits_what_it_does_not_know() {
    let (dir, hito, actor, library, music) = support::library("facts-host").await;
    let source = dir.path().join("source.wav");
    support::write_wav(&source, 0);
    let path = music.join("plain.mp3");
    encode(
        &source,
        &path,
        &[
            "-c:a",
            "libmp3lame",
            "-write_id3v2",
            "0",
            "-write_xing",
            "0",
        ],
    );
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Plain".into());
    tag.set_artist("Plain artist".into());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    let detail = only_recording_detail(&hito, &actor).await;
    let format = detail.audio_format.expect("the ready file has a format");
    assert_eq!(format.format.as_deref(), Some("MP3"));
    assert_eq!(format.bit_depth, None);
    assert_eq!(detail.release_facts.year, None);
    assert_eq!(detail.release_facts.track_total, None);
    assert_eq!(detail.release_facts.label, None);
    assert!(detail.last_played.is_none());
    assert!(detail.appears_in.is_empty());
}

#[tokio::test]
async fn last_played_and_playlist_appearances_belong_to_the_signed_in_account() {
    let (_dir, hito, actor, library) = support::catalog().await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording = Uuid::parse_str(&track.id).unwrap();
    let bob = hito
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "bob".into(),
                password: "facts bob password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&actor, &library, &bob.id, true)
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "facts bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let late = Uuid::new_v4();
    let road = Uuid::new_v4();
    let empty = Uuid::new_v4();
    let removed = Uuid::new_v4();
    let removed_entry = Uuid::new_v4();
    let mut actions = vec![Action::ProgressSet {
        recording_id: recording,
        position_ms: 97_000,
        speed: 1.0,
    }];
    for (playlist, name) in [
        (road, "Road trip"),
        (late, "Late night"),
        (empty, "Empty"),
        (removed, "Removed"),
    ] {
        actions.push(Action::PlaylistCreate {
            playlist_id: playlist,
            name: name.into(),
        });
    }
    for (playlist, entry) in [
        (road, Uuid::new_v4()),
        (late, Uuid::new_v4()),
        (removed, removed_entry),
    ] {
        actions.push(Action::EntryInsert {
            playlist_id: playlist,
            entry_id: entry,
            recording_id: recording,
            position: 0,
        });
    }
    actions.push(Action::EntryRemove {
        playlist_id: removed,
        entry_id: removed_entry,
    });
    support::synchronize(&hito, &actor, actions).await;
    let own = hito
        .catalog
        .recording_metadata(&actor, &track.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    let played = own.last_played.expect("a resume position exists");
    assert_eq!(played.position_ms, 97_000);
    assert!(played.played_at > 0);
    let names: Vec<_> = own.appears_in.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Late night", "Road trip"]);
    let other = hito
        .catalog
        .recording_metadata(&bob, &track.id, MetadataHistoryQuery::default())
        .await
        .unwrap();
    assert!(other.last_played.is_none());
    assert!(other.appears_in.is_empty());
}
