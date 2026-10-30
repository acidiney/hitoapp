use hito_core::test_support::support;
use hito_core::{Action, Browse, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use sqlx::Connection;
use std::{path::Path, process::Command};
use uuid::Uuid;

const R1: &str = "b1000000-0000-4000-8000-000000000001";
const R2: &str = "b1000000-0000-4000-8000-000000000002";
const G1: &str = "b1000000-0000-4000-8000-0000000000a1";
const REC1: &str = "b1000000-0000-4000-8000-0000000000c1";
const REC2: &str = "b1000000-0000-4000-8000-0000000000c2";
const TRACK1: &str = "b1000000-0000-4000-8000-0000000000d1";

struct Tagged<'a> {
    file: &'a str,
    title: &'a str,
    album: &'a str,
    release: Option<&'a str>,
    group: Option<&'a str>,
    recording: Option<&'a str>,
    release_track: Option<&'a str>,
    offset: i16,
}

fn write(directory: &Path, music: &Path, tagged: &Tagged<'_>) {
    let source = directory.join(format!("{}.wav", tagged.file));
    support::write_wav(&source, tagged.offset);
    let path = music.join(format!("{}.flac", tagged.file));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(["-c:a", "flac"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title(tagged.title.into());
    tag.set_artist("Tagged credit".into());
    tag.set_album(tagged.album.into());
    for (key, value) in [
        (ItemKey::MusicBrainzReleaseId, tagged.release),
        (ItemKey::MusicBrainzReleaseGroupId, tagged.group),
        (ItemKey::MusicBrainzRecordingId, tagged.recording),
        (ItemKey::MusicBrainzTrackId, tagged.release_track),
    ] {
        if let Some(value) = value {
            assert!(tag.insert_text(key, value.into()));
        }
    }
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
}

type Row = (String, String, String, Option<String>);

async fn identities(directory: &Path) -> Vec<Row> {
    let mut connection = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}?mode=ro",
        directory.join("hito.db").display()
    ))
    .await
    .unwrap();
    sqlx::query_as(
        "SELECT r.title,i.entity,i.mbid,i.release_id FROM recording_musicbrainz_observations i
         JOIN recordings r ON r.id=i.recording_id ORDER BY r.title,i.entity",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap()
}

async fn release_ids(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
) -> Vec<(String, String)> {
    let mut releases: Vec<(String, String)> = hito
        .catalog
        .releases(actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|release| (release.title, release.id))
        .collect();
    releases.sort();
    releases
}

#[tokio::test]
async fn valid_release_and_recording_ids_become_identity_evidence_without_merging_releases() {
    let (dir, hito, actor, library, music) = support::library("musicbrainz-identity-host").await;
    let shared = dir.path();
    let tracks = [
        Tagged {
            file: "one",
            title: "First",
            album: "Album One",
            release: Some(R1),
            group: Some(G1),
            recording: Some(REC1),
            release_track: Some(TRACK1),
            offset: 1,
        },
        Tagged {
            file: "two",
            title: "Second",
            album: "Album Two",
            release: Some(R1),
            group: None,
            recording: Some(REC2),
            release_track: None,
            offset: 2,
        },
        Tagged {
            file: "bad",
            title: "Third",
            album: "Album Three",
            release: Some("not-a-musicbrainz-id"),
            group: None,
            recording: Some("B1000000-0000-4000-8000-0000000000C3"),
            release_track: None,
            offset: 3,
        },
        Tagged {
            file: "plain",
            title: "Fourth",
            album: "Album Four",
            release: None,
            group: None,
            recording: None,
            release_track: None,
            offset: 4,
        },
    ];
    for track in &tracks {
        write(shared, &music, track);
    }
    support::scan(&hito, &actor, &library).await;

    let releases = release_ids(&hito, &actor).await;
    assert_eq!(
        releases
            .iter()
            .map(|(title, _)| title.as_str())
            .collect::<Vec<_>>(),
        ["Album Four", "Album One", "Album Three", "Album Two"],
        "Releases sharing an identifier stay distinct"
    );
    let release = |title: &str| -> Option<String> {
        releases
            .iter()
            .find(|(name, _)| name == title)
            .map(|(_, id)| id.clone())
    };
    let row = |title: &str, entity: &str, mbid: &str, album: &str| -> Row {
        (title.into(), entity.into(), mbid.into(), release(album))
    };
    assert_eq!(
        identities(shared).await,
        vec![
            row("First", "recording", REC1, "Album One"),
            row("First", "release", R1, "Album One"),
            row("First", "release_group", G1, "Album One"),
            row("First", "release_track", TRACK1, "Album One"),
            row("Second", "recording", REC2, "Album Two"),
            row("Second", "release", R1, "Album Two"),
            row(
                "Third",
                "recording",
                "b1000000-0000-4000-8000-0000000000c3",
                "Album Three"
            ),
        ],
        "Only well-formed identifiers are evidence, stored in lower case"
    );

    let again = identities(shared).await;
    support::scan(&hito, &actor, &library).await;
    assert_eq!(
        identities(shared).await,
        again,
        "A second scan adds nothing"
    );
}

#[tokio::test]
async fn identity_evidence_never_overrides_a_personal_edit() {
    let (dir, hito, actor, library, music) = support::library("musicbrainz-identity-edit").await;
    write(
        dir.path(),
        &music,
        &Tagged {
            file: "one",
            title: "First",
            album: "Album One",
            release: Some(R2),
            group: None,
            recording: Some(REC1),
            release_track: None,
            offset: 1,
        },
    );
    support::scan(&hito, &actor, &library).await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleSet {
            recording_id: Uuid::parse_str(&recording.id).unwrap(),
            title: "Personal title".into(),
        }],
    )
    .await;
    support::scan(&hito, &actor, &library).await;
    let page = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.items[0].title, "Personal title");
    assert_eq!(identities(dir.path()).await.len(), 2);
}
