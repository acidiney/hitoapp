use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    support,
};
use hito_core::{
    Actor, ArtistAssociationEvidence, MatchArtistsFromProviderEvidence, MetadataProvider,
    ResetRecordingArtistIdentity, Services, UpdateAutomaticLookups,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

const ARTIST: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";
const ONE: &str = "0f700000-0000-4000-8000-0000000000b1";
const TWO_ARTISTS: &str = "0f700000-0000-4000-8000-0000000000b2";
const UNKNOWN: &str = "0f700000-0000-4000-8000-0000000000b3";
const OUTAGE: &str = "0f700000-0000-4000-8000-0000000000b4";
const RESET: &str = "0f700000-0000-4000-8000-0000000000b5";
const SAME_CREDIT: &str = "0f700000-0000-4000-8000-0000000000b6";

struct World {
    hito: Services,
    actor: Actor,
    fake: FakeMusicBrainz,
    ids: HashMap<String, String>,
    _directory: tempfile::TempDir,
}

fn flac(directory: &std::path::Path, music: &Path, offset: i16, title: &str, recording: &str) {
    let source = directory.join(format!("{title}.wav"));
    support::write_wav(&source, offset);
    let path = music.join(format!("{title}.flac"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(["-c:a", "flac"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title(title.into());
    tag.set_artist("Performer".into());
    tag.insert_text(ItemKey::MusicBrainzRecordingId, recording.into());
    tag.save_to_path(&path, Default::default()).unwrap();
}

impl World {
    async fn prepare() -> Self {
        let fake = FakeMusicBrainz::start().await;
        fake.recording(
            ONE,
            Reply::Recordings(vec![(ONE.into(), vec![ARTIST.into()])]),
        );
        fake.recording(
            TWO_ARTISTS,
            Reply::Recordings(vec![(
                TWO_ARTISTS.into(),
                vec![ARTIST.into(), OTHER.into()],
            )]),
        );
        fake.recording(UNKNOWN, Reply::NotFound);
        fake.recording(OUTAGE, Reply::Unavailable);
        fake.recording(
            RESET,
            Reply::Recordings(vec![(RESET.into(), vec![ARTIST.into()])]),
        );
        fake.recording(
            SAME_CREDIT,
            Reply::Recordings(vec![(SAME_CREDIT.into(), vec![ARTIST.into()])]),
        );
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz("recording-id", Some(fake.endpoint.clone())).await;
        for (offset, title, recording) in [
            (1, "One", ONE),
            (2, "Two Artists", TWO_ARTISTS),
            (3, "Unknown", UNKNOWN),
            (4, "Outage", OUTAGE),
            (5, "Reset", RESET),
            (6, "Malformed", "not-a-recording-id"),
        ] {
            flac(directory.path(), &music, offset, title, recording);
        }
        support::scan(&hito, &actor, &library).await;
        let ids = hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|item| (item.title, item.id))
            .collect();
        Self {
            hito,
            actor,
            fake,
            ids,
            _directory: directory,
        }
    }

    async fn run(&self) -> hito_core::ArtistProviderMatchReceipt {
        self.hito
            .artist_catalog
            .match_artists_from_provider_evidence(
                &self.actor,
                MatchArtistsFromProviderEvidence {
                    request_id: Uuid::new_v4(),
                    after: None,
                    limit: None,
                },
            )
            .await
            .unwrap()
    }

    async fn associations(&self, title: &str) -> Vec<hito_core::ArtistAssociation> {
        self.hito
            .artist_catalog
            .recording_artist_associations(&self.actor, &self.ids[title])
            .await
            .unwrap()
    }

    fn recording_requests(&self) -> Vec<String> {
        self.fake
            .requests()
            .into_iter()
            .filter(|request| request.starts_with("/ws/2/recording/"))
            .collect()
    }
}

#[tokio::test]
async fn a_recording_id_in_the_file_tags_matches_the_single_credited_artist() {
    let world = World::prepare().await;
    assert!(
        world.fake.requests().is_empty(),
        "Scanning never contacts MusicBrainz"
    );
    let receipt = world.run().await;
    assert_eq!(receipt.matched, 2, "{receipt:?}");
    assert_eq!(receipt.unavailable, 1);

    let linked = world.associations("One").await;
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].provider, MetadataProvider::BuiltinMusicBrainz);
    assert_eq!(linked[0].provider_artist_id, ARTIST);
    assert_eq!(
        linked[0].evidence,
        ArtistAssociationEvidence::MatchedRecordingId
    );
    for title in ["Two Artists", "Unknown", "Outage", "Malformed"] {
        assert!(world.associations(title).await.is_empty(), "{title}");
    }
    let requests = world.recording_requests();
    assert!(
        requests.contains(&format!(
            "/ws/2/recording/{ONE}?fmt=json&inc=artist-credits"
        )),
        "{requests:?}"
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("not-a-recording-id")),
        "A malformed tag value is never asked about"
    );
}

#[tokio::test]
async fn settled_answers_are_not_asked_again_but_an_outage_is() {
    let world = World::prepare().await;
    world.run().await;
    let before = world.recording_requests().len();
    let second = world.run().await;
    assert_eq!((second.matched, second.unavailable), (0, 1));
    assert_eq!(
        world.recording_requests().len(),
        before + 1,
        "Only the unavailable answer is asked again"
    );
}

#[tokio::test]
async fn a_personally_reset_recording_is_never_looked_up_or_overridden() {
    let world = World::prepare().await;
    let choice = world
        .hito
        .artist_catalog
        .recording_artist_identity_choice(&world.actor, &world.ids["Reset"])
        .await
        .unwrap();
    world
        .hito
        .artist_catalog
        .reset_recording_artist_identity(
            &world.actor,
            &world.ids["Reset"],
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    world.run().await;
    assert!(world.associations("Reset").await.is_empty());
    assert!(
        !world
            .recording_requests()
            .iter()
            .any(|request| request.contains(RESET))
    );
}

impl World {
    async fn walk(&self) {
        let mut steps = 0;
        while self
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
        {
            steps += 1;
            assert!(steps < 40, "The automatic walk must end");
        }
    }

    async fn allow_lookups(&self, allowed: bool) {
        self.hito
            .artist_catalog
            .set_automatic_lookups(
                &self.actor,
                UpdateAutomaticLookups {
                    automatic_artist_lookup: Some(allowed),
                    automatic_artwork: None,
                    automatic_service_search: None,
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn the_automatic_walk_matches_tagged_recordings_without_a_click() {
    let world = World::prepare().await;
    world.walk().await;
    let linked = world.associations("One").await;
    assert_eq!(linked.len(), 1);
    assert_eq!(
        linked[0].evidence,
        ArtistAssociationEvidence::MatchedRecordingId
    );
}

#[tokio::test]
async fn the_automatic_walk_asks_nothing_while_the_account_forbids_lookups() {
    let world = World::prepare().await;
    world.allow_lookups(false).await;
    world.walk().await;
    assert!(world.recording_requests().is_empty());
    assert!(world.associations("One").await.is_empty());
}
