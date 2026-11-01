use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    support,
};
use hito_core::{ArtistAssociationEvidence, MatchArtistsFromProviderEvidence, Search, Services};
use lofty::{
    config::WriteOptions,
    id3::v2::{Frame, Id3v2Tag, UniqueFileIdentifierFrame},
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use std::path::Path;
use uuid::Uuid;

#[path = "../fixtures/metadata_tags_v2.rs"]
mod prior_inspection;

const ARTIST: &str = "11111111-1111-4111-8111-111111111111";
const SONG: &str = "0f700000-0000-4000-8000-0000000000e1";

fn tagged(directory: &Path, music: &Path, name: &str, offset: i16, title: &str, recording: &str) {
    let source = directory.join(format!("{name}.source.wav"));
    support::write_wav(&source, offset);
    let (extension, encoder): (&str, &[&str]) = if name.ends_with("mp3") {
        (
            "mp3",
            &[
                "-c:a",
                "libmp3lame",
                "-write_id3v2",
                "0",
                "-write_xing",
                "0",
            ],
        )
    } else {
        ("wav", &["-c:a", "pcm_s16le"])
    };
    let path = music.join(format!("{name}.{extension}"));
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(&source)
            .args(encoder)
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Performer".into());
    let mut id3 = Id3v2Tag::from(tag);
    id3.insert(Frame::UniqueFileIdentifier(UniqueFileIdentifierFrame::new(
        "http://musicbrainz.org".into(),
        recording.as_bytes().to_vec(),
    )));
    id3.save_to_path(&path, WriteOptions::default()).unwrap();
}

async fn match_all(
    hito: &Services,
    actor: &hito_core::Actor,
) -> hito_core::ArtistProviderMatchReceipt {
    hito.artist_catalog
        .match_artists_from_provider_evidence(
            actor,
            MatchArtistsFromProviderEvidence {
                request_id: Uuid::new_v4(),
                after: None,
                limit: None,
            },
        )
        .await
        .unwrap()
}

async fn walk(hito: &Services) {
    let mut steps = 0;
    while hito
        .artist_catalog
        .run_next_automatic_matching_until(std::future::pending())
        .await
        .unwrap()
    {
        steps += 1;
        assert!(steps < 40, "The automatic walk must end");
    }
}

#[tokio::test]
async fn mp3_and_wav_ufid_recording_ids_match_the_credited_artist_and_group_the_copies() {
    let fake = FakeMusicBrainz::start().await;
    fake.recording(
        SONG,
        Reply::Recordings(vec![(SONG.into(), vec![ARTIST.into()])]),
    );
    let (dir, hito, actor, library, music) =
        support::library_with_musicbrainz("ufid-match", Some(fake.endpoint.clone())).await;
    tagged(dir.path(), &music, "a-mp3", 1, "Shared Song", SONG);
    tagged(dir.path(), &music, "b-wav", 2, "Shared Song", SONG);
    support::scan(&hito, &actor, &library).await;
    assert!(
        fake.requests().is_empty(),
        "Scanning never contacts MusicBrainz"
    );
    let receipt = match_all(&hito, &actor).await;
    assert_eq!(receipt.matched, 2, "{receipt:?}");
    walk(&hito).await;
    let cards = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(cards.items.len(), 1, "one card for the song in two formats");
    let copies = hito
        .catalog
        .recording_copies(&actor, &cards.items[0].id)
        .await
        .unwrap();
    assert_eq!(copies.copies.len(), 2);
    for copy in &copies.copies {
        let linked = hito
            .artist_catalog
            .recording_artist_associations(&actor, &copy.recording_id)
            .await
            .unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].provider_artist_id, ARTIST);
        assert_eq!(
            linked[0].evidence,
            ArtistAssociationEvidence::MatchedRecordingId
        );
    }
}

#[tokio::test]
async fn an_existing_mp3_library_is_inspected_again_and_matched_after_the_upgrade() {
    let fake = FakeMusicBrainz::start().await;
    fake.recording(SONG, Reply::NotFound);
    let (dir, hito, actor, library, music) =
        support::library_with_musicbrainz("ufid-upgrade", Some(fake.endpoint.clone())).await;
    tagged(dir.path(), &music, "old-mp3", 1, "Old Song", SONG);
    support::scan(&hito, &actor, &library).await;
    walk(&hito).await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert!(
        hito.artist_catalog
            .recording_artist_associations(&actor, &recording.id)
            .await
            .unwrap()
            .is_empty()
    );
    let source: String = {
        let pool =
            sqlx::SqlitePool::connect(&format!("sqlite:{}", dir.path().join("hito.db").display()))
                .await
                .unwrap();
        let id = sqlx::query_scalar("SELECT source_id FROM source_metadata")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        id
    };
    prior_inspection::represent_prior_inspection(&dir.path().join("hito.db"), &source).await;
    fake.recording(
        SONG,
        Reply::Recordings(vec![(SONG.into(), vec![ARTIST.into()])]),
    );

    support::scan(&hito, &actor, &library).await;
    walk(&hito).await;
    let linked = hito
        .artist_catalog
        .recording_artist_associations(&actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].provider_artist_id, ARTIST);
    assert_eq!(
        linked[0].evidence,
        ArtistAssociationEvidence::MatchedRecordingId
    );
}
