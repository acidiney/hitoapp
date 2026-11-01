use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    support,
};
use hito_core::{Action, Actor, Credentials, RecordingGroupProof, Search, Services};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::path::Path;
use uuid::Uuid;

const MBID: &str = "0f700000-0000-4000-8000-0000000000d1";
const OTHER_MBID: &str = "0f700000-0000-4000-8000-0000000000d2";
const ISRC: &str = "GBAAA2600001";

#[derive(Clone, Copy, Default)]
struct Identity<'a> {
    recording: Option<&'a str>,
    isrc: Option<&'a str>,
}

struct World {
    hito: Services,
    actor: Actor,
    library: String,
    _fake: FakeMusicBrainz,
    _directory: tempfile::TempDir,
}

fn flac(
    directory: &Path,
    music: &Path,
    file: &str,
    offset: i16,
    title: &str,
    seconds: f32,
    identity: Identity<'_>,
) {
    let source = directory.join(format!("{file}.wav"));
    support::write_wav(&source, offset);
    let path = music.join(format!("{file}.flac"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(["-t", &seconds.to_string(), "-af", "apad", "-c:a", "flac"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title(title.into());
    tag.set_artist("Performer".into());
    if let Some(recording) = identity.recording {
        tag.insert_text(ItemKey::MusicBrainzRecordingId, recording.into());
    }
    if let Some(isrc) = identity.isrc {
        tag.insert_text(ItemKey::Isrc, isrc.into());
    }
    tag.save_to_path(&path, Default::default()).unwrap();
}

impl World {
    async fn prepare(files: &[(&str, &str, f32, Identity<'_>)]) -> Self {
        let fake = FakeMusicBrainz::start().await;
        for id in [MBID, OTHER_MBID] {
            fake.recording(id, Reply::NotFound);
        }
        fake.isrc(ISRC, Reply::NotFound);
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz("groups", Some(fake.endpoint.clone())).await;
        for (index, (file, title, seconds, identity)) in files.iter().enumerate() {
            flac(
                directory.path(),
                &music,
                file,
                index as i16 + 1,
                title,
                *seconds,
                *identity,
            );
        }
        support::scan(&hito, &actor, &library).await;
        Self {
            hito,
            actor,
            library,
            _fake: fake,
            _directory: directory,
        }
    }

    async fn with(files: &[(&str, &str, f32, Identity<'_>)]) -> Self {
        let world = Self::prepare(files).await;
        world.walk().await;
        world
    }

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
            assert!(steps < 80, "The automatic walk must end");
        }
    }

    async fn rescan_and_walk(&self) {
        support::scan(&self.hito, &self.actor, &self.library).await;
        self.walk().await;
    }

    async fn cards_of(&self, actor: &Actor) -> Vec<hito_core::Recording> {
        let page = self
            .hito
            .catalog
            .recordings(actor, Search::default())
            .await
            .unwrap();
        assert_eq!(page.total as usize, page.items.len());
        page.items
    }

    async fn cards(&self) -> Vec<hito_core::Recording> {
        self.cards_of(&self.actor).await
    }

    async fn copies(&self, recording: &str) -> hito_core::RecordingCopies {
        self.hito
            .catalog
            .recording_copies(&self.actor, recording)
            .await
            .unwrap()
    }

    async fn act(&self, actions: Vec<Action>) {
        support::synchronize(&self.hito, &self.actor, actions).await;
    }
}

fn tagged(recording: &str) -> Identity<'_> {
    Identity {
        recording: Some(recording),
        isrc: None,
    }
}

fn coded(isrc: &str) -> Identity<'_> {
    Identity {
        recording: None,
        isrc: Some(isrc),
    }
}

fn uuid(id: &str) -> Uuid {
    Uuid::parse_str(id).unwrap()
}

#[tokio::test]
async fn the_same_tagged_recording_shows_one_card_and_lists_every_copy() {
    let world = World::with(&[
        ("a", "Shared Song", 1.0, tagged(MBID)),
        ("b", "Shared Song", 1.0, tagged(MBID)),
        ("c", "Other Song", 1.0, tagged(OTHER_MBID)),
    ])
    .await;
    let cards = world.cards().await;
    assert_eq!(
        cards.len(),
        2,
        "one card for the pair and one for the other song"
    );
    let shared = cards
        .iter()
        .find(|card| card.title == "Shared Song")
        .unwrap();
    let copies = world.copies(&shared.id).await;
    assert_eq!(copies.canonical_id, shared.id);
    assert_eq!(copies.copies.len(), 2);
    assert_eq!(
        copies.copies.iter().filter(|copy| copy.canonical).count(),
        1
    );
    let member = copies.copies.iter().find(|copy| !copy.canonical).unwrap();
    assert_eq!(
        member.proof,
        Some(RecordingGroupProof::MusicbrainzRecording)
    );
    for copy in &copies.copies {
        assert_eq!(copy.sources.len(), 1, "each copy keeps its own source");
        assert!(copy.sources[0].ready);
        assert_eq!(copy.sources[0].recording_id, copy.recording_id);
    }
    assert_eq!(
        world.copies(&member.recording_id).await.canonical_id,
        shared.id
    );
    let found = world
        .hito
        .catalog
        .recordings_by_ids(&world.actor, &[uuid(&member.recording_id)])
        .await
        .unwrap();
    assert_eq!(
        found.len(),
        1,
        "the grouped recording itself is still readable"
    );
}

#[tokio::test]
async fn the_same_isrc_groups_and_a_second_walk_changes_nothing() {
    let world = World::with(&[
        ("a", "Coded", 1.0, coded(ISRC)),
        ("b", "Coded", 1.0, coded(ISRC)),
    ])
    .await;
    let cards = world.cards().await;
    assert_eq!(cards.len(), 1);
    let copies = world.copies(&cards[0].id).await;
    assert_eq!(
        copies
            .copies
            .iter()
            .find(|copy| !copy.canonical)
            .unwrap()
            .proof,
        Some(RecordingGroupProof::Isrc)
    );
    world.rescan_and_walk().await;
    let again = world.copies(&cards[0].id).await;
    assert_eq!(world.cards().await.len(), 1);
    assert_eq!(again.canonical_id, copies.canonical_id);
    assert_eq!(again.copies.len(), 2);
}

#[tokio::test]
async fn equal_title_artist_and_length_without_proof_stay_separate() {
    let world = World::with(&[
        ("a", "Twin", 1.0, Identity::default()),
        ("b", "Twin", 1.0, Identity::default()),
        ("c", "Tagged Twin", 1.0, tagged(MBID)),
        ("d", "Tagged Twin", 1.0, tagged(OTHER_MBID)),
    ])
    .await;
    assert_eq!(world.cards().await.len(), 4);
}

#[tokio::test]
async fn versions_and_different_lengths_with_one_identifier_are_not_the_same_song() {
    let world = World::with(&[
        ("a", "Song", 1.0, coded(ISRC)),
        ("b", "Song (Live)", 1.0, coded(ISRC)),
        ("c", "Song (Remix)", 1.0, coded(ISRC)),
        ("d", "Song", 4.0, coded(ISRC)),
    ])
    .await;
    assert_eq!(world.cards().await.len(), 4);
}

#[tokio::test]
async fn the_copy_with_a_personal_like_carries_the_card() {
    let world = World::prepare(&[
        ("a", "Liked Twin", 1.0, tagged(MBID)),
        ("b", "Liked Twin", 1.0, tagged(MBID)),
    ])
    .await;
    let mut ids: Vec<String> = world
        .cards()
        .await
        .into_iter()
        .map(|card| card.id)
        .collect();
    ids.sort();
    let liked = ids.last().unwrap().clone();
    world
        .act(vec![Action::LikeSet {
            recording_id: uuid(&liked),
            liked: true,
        }])
        .await;
    world.walk().await;
    let cards = world.cards().await;
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, liked);
    assert!(cards[0].liked);
    let favourites = world
        .hito
        .catalog
        .liked_recordings(&world.actor, Search::default())
        .await
        .unwrap();
    assert_eq!(favourites.total, 1);
    assert_eq!(world.copies(&liked).await.copies.len(), 2);
}

#[tokio::test]
async fn different_personal_decisions_on_each_copy_leave_two_cards() {
    let world = World::prepare(&[
        ("a", "Decided", 1.0, tagged(MBID)),
        ("b", "Decided", 1.0, tagged(MBID)),
    ])
    .await;
    let mut ids: Vec<String> = world
        .cards()
        .await
        .into_iter()
        .map(|card| card.id)
        .collect();
    ids.sort();
    world
        .act(vec![
            Action::LikeSet {
                recording_id: uuid(&ids[0]),
                liked: true,
            },
            Action::RecordingTitleSet {
                recording_id: uuid(&ids[1]),
                title: "My name for it".into(),
            },
        ])
        .await;
    world.walk().await;
    assert_eq!(world.cards().await.len(), 2);
    assert!(world.copies(&ids[0]).await.copies.is_empty());
}

#[tokio::test]
async fn splitting_keeps_every_copy_a_card_of_its_own_and_survives_the_next_walk() {
    let world = World::with(&[
        ("a", "Split Me", 1.0, tagged(MBID)),
        ("b", "Split Me", 1.0, tagged(MBID)),
    ])
    .await;
    let cards = world.cards().await;
    assert_eq!(cards.len(), 1);
    let copies = world.copies(&cards[0].id).await;
    let member = copies
        .copies
        .iter()
        .find(|copy| !copy.canonical)
        .unwrap()
        .recording_id
        .clone();
    let split = world
        .hito
        .catalog
        .split_recording_copy(&world.actor, &member)
        .await
        .unwrap();
    assert!(split.separate);
    assert_eq!(world.cards().await.len(), 2);
    let replay = world
        .hito
        .catalog
        .split_recording_copy(&world.actor, &member)
        .await
        .unwrap();
    assert!(replay.separate, "a retried split succeeds again");
    world.rescan_and_walk().await;
    assert_eq!(world.cards().await.len(), 2);
    assert!(world.copies(&member).await.copies.is_empty());
}

#[tokio::test]
async fn a_split_and_a_group_belong_to_one_account() {
    let world = World::with(&[
        ("a", "Mine", 1.0, tagged(MBID)),
        ("b", "Mine", 1.0, tagged(MBID)),
    ])
    .await;
    let bob = world
        .hito
        .identity
        .create_user(
            &world.actor,
            Credentials {
                username: "bob".into(),
                password: "groups bob password".into(),
            },
        )
        .await
        .unwrap();
    world
        .hito
        .identity
        .set_library_access(&world.actor, &world.library, &bob.id, true)
        .await
        .unwrap();
    let session = world
        .hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "groups bob password".into(),
        })
        .await
        .unwrap();
    let bob = world
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert_eq!(
        world.cards_of(&bob).await.len(),
        2,
        "Bob has not been walked yet"
    );
    let member = {
        let cards = world.cards().await;
        world
            .copies(&cards[0].id)
            .await
            .copies
            .into_iter()
            .find(|copy| !copy.canonical)
            .unwrap()
            .recording_id
    };
    world
        .hito
        .catalog
        .split_recording_copy(&world.actor, &member)
        .await
        .unwrap();
    world
        .hito
        .artist_catalog
        .queue_first_automatic_matching()
        .await
        .unwrap();
    world.walk().await;
    assert_eq!(world.cards_of(&bob).await.len(), 1);
    assert_eq!(world.cards().await.len(), 2);
    assert!(
        world
            .hito
            .catalog
            .split_recording_copy(&bob, &member)
            .await
            .unwrap()
            .separate
    );
    assert_eq!(world.cards_of(&bob).await.len(), 2);
}
