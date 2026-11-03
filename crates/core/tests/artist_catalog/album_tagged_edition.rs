use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    support,
};
use hito_core::{
    Actor, Browse, ChooseAlbumEdition, ClearAlbumEdition, Credentials,
    MatchArtistsFromProviderEvidence, Services, TaggedAlbumEdition, UpdateAutomaticLookups,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::{path::Path, time::Duration};
use uuid::Uuid;

const ARTIST: &str = "11111111-1111-4111-8111-111111111111";
const RELEASE: &str = "0f700000-0000-4000-8000-0000000000a1";
const OTHER_RELEASE: &str = "0f700000-0000-4000-8000-0000000000a2";
const GROUP: &str = "0f700000-0000-4000-8000-0000000000b1";
const TAGGED_GROUP: &str = "0f700000-0000-4000-8000-0000000000b2";

struct Track {
    release: Option<&'static str>,
    group: Option<&'static str>,
}

fn same(release: &'static str, group: Option<&'static str>) -> Vec<Track> {
    (0..3)
        .map(|_| Track {
            release: Some(release),
            group,
        })
        .collect()
}

struct World {
    hito: Services,
    actor: Actor,
    fake: FakeMusicBrainz,
    release_id: String,
    recordings: Vec<String>,
    _directory: tempfile::TempDir,
}

fn flac(directory: &Path, music: &Path, offset: i16, index: usize, track: &Track, recording: &str) {
    let source = directory.join(format!("{index}.wav"));
    support::write_wav(&source, offset);
    let path = music.join(format!("{index}.flac"));
    let status = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(["-c:a", "flac"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title(format!("Song {index}"));
    tag.set_artist("Performer".into());
    tag.set_album("Tagged Album".into());
    tag.insert_text(ItemKey::TrackNumber, (index + 1).to_string());
    tag.insert_text(ItemKey::MusicBrainzRecordingId, recording.into());
    if let Some(release) = track.release {
        tag.insert_text(ItemKey::MusicBrainzReleaseId, release.into());
    }
    if let Some(group) = track.group {
        tag.insert_text(ItemKey::MusicBrainzReleaseGroupId, group.into());
    }
    tag.save_to_path(&path, Default::default()).unwrap();
}

fn recording_id(index: usize) -> String {
    format!("0f700000-0000-4000-8000-0000000000c{index}")
}

impl World {
    async fn prepare(tracks: Vec<Track>, matched: bool) -> Self {
        let fake = FakeMusicBrainz::start().await;
        for index in 0..tracks.len() {
            fake.recording(
                &recording_id(index),
                Reply::Recordings(vec![(recording_id(index), vec![ARTIST.into()])]),
            );
        }
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz("tagged-edition", Some(fake.endpoint.clone())).await;
        for (index, track) in tracks.iter().enumerate() {
            flac(
                directory.path(),
                &music,
                index as i16 + 1,
                index,
                track,
                &recording_id(index),
            );
        }
        support::scan(&hito, &actor, &library).await;
        if matched {
            hito.artist_catalog
                .match_artists_from_provider_evidence(
                    &actor,
                    MatchArtistsFromProviderEvidence {
                        request_id: Uuid::new_v4(),
                        after: None,
                        limit: None,
                    },
                )
                .await
                .unwrap();
        }
        let release_id = hito
            .catalog
            .releases(&actor, Browse::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let recordings = hito
            .catalog
            .release(&actor, &release_id, Browse::default())
            .await
            .unwrap()
            .tracks
            .items
            .into_iter()
            .map(|track| track.recording.id)
            .collect();
        Self {
            hito,
            actor,
            fake,
            release_id,
            recordings,
            _directory: directory,
        }
    }

    async fn lookup(&self) -> TaggedAlbumEdition {
        self.hito
            .artist_catalog
            .tagged_album_edition_until(&self.actor, &self.release_id, std::future::pending())
            .await
            .unwrap()
    }

    fn release_requests(&self) -> Vec<String> {
        self.fake
            .requests()
            .into_iter()
            .filter(|request| request.starts_with("/ws/2/release/"))
            .collect()
    }

    async fn consent(&self, allowed: bool) {
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

    async fn association(&self) -> (String, String) {
        let recording = self.recordings[0].clone();
        let association = self
            .hito
            .artist_catalog
            .recording_artist_associations(&self.actor, &recording)
            .await
            .unwrap()
            .remove(0);
        (recording, association.id)
    }
}

fn found(edition: TaggedAlbumEdition) -> hito_core::SavedAlbumEdition {
    match edition {
        TaggedAlbumEdition::Found { edition } => edition,
        TaggedAlbumEdition::None => panic!("The tagged release should have been found"),
    }
}

#[tokio::test]
async fn a_tagged_album_opens_on_its_exact_edition_after_one_release_request() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    assert!(
        world.release_requests().is_empty(),
        "Scanning and matching never look up the release"
    );
    let edition = found(world.lookup().await);
    assert_eq!(edition.edition_id, RELEASE);
    assert_eq!(edition.release_group_id, GROUP);
    let (recording, association) = world.association().await;
    assert_eq!(edition.association_id, association);
    assert!(world.recordings.contains(&edition.recording_id));
    assert_eq!(edition.recording_id, recording);
    assert_eq!(
        world.release_requests(),
        vec![format!(
            "/ws/2/release/{RELEASE}?fmt=json&inc=release-groups"
        )]
    );
    found(world.lookup().await);
    assert_eq!(
        world.release_requests().len(),
        2,
        "Each lookup makes exactly one request"
    );
}

#[tokio::test]
async fn a_tagged_release_group_is_checked_against_musicbrainz_and_never_trusted() {
    let world = World::prepare(same(RELEASE, Some(TAGGED_GROUP)), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    let edition = found(world.lookup().await);
    assert_eq!(edition.edition_id, RELEASE);
    assert_eq!(edition.release_group_id, GROUP);
    assert_eq!(world.release_requests().len(), 1);
    world.fake.release(RELEASE, Reply::NotFound);
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
}

#[tokio::test]
async fn another_account_cannot_use_an_album_it_cannot_open() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    let credentials = || Credentials {
        username: "bob".into(),
        password: "tagged bob password".into(),
    };
    world
        .hito
        .identity
        .create_user(&world.actor, credentials())
        .await
        .unwrap();
    let session = world.hito.identity.login(credentials()).await.unwrap();
    let bob = world
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        world
            .hito
            .artist_catalog
            .tagged_album_edition_until(&bob, &world.release_id, std::future::pending())
            .await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(world.release_requests().is_empty());
    found(world.lookup().await);
    assert_eq!(world.release_requests().len(), 1);
}

#[tokio::test]
async fn a_busy_lookup_slot_falls_back_without_a_second_request() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    world.fake.delay_replies(Duration::from_secs(1));
    let (first, second) = tokio::join!(world.lookup(), world.lookup());
    let results = [first, second];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, TaggedAlbumEdition::Found { .. }))
            .count(),
        1
    );
    assert!(results.contains(&TaggedAlbumEdition::None));
    assert_eq!(world.release_requests().len(), 1);
}

#[tokio::test]
async fn a_lookup_that_never_answers_gives_up_within_the_request_budget() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    world.fake.delay_replies(Duration::from_secs(600));
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
    let elapsed = started.elapsed();
    assert!(elapsed <= Duration::from_secs(16), "{elapsed:?}");
}

#[tokio::test]
async fn tracks_that_disagree_or_lack_the_tag_fall_back_without_a_request() {
    for tracks in [
        vec![
            Track {
                release: Some(RELEASE),
                group: None,
            },
            Track {
                release: Some(RELEASE),
                group: None,
            },
            Track {
                release: Some(OTHER_RELEASE),
                group: None,
            },
        ],
        vec![
            Track {
                release: Some(RELEASE),
                group: None,
            },
            Track {
                release: None,
                group: None,
            },
            Track {
                release: Some(RELEASE),
                group: None,
            },
        ],
    ] {
        let world = World::prepare(tracks, true).await;
        world
            .fake
            .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
        assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
        assert!(world.release_requests().is_empty());
    }
}

#[tokio::test]
async fn an_owner_choice_wins_over_the_tag_even_after_it_was_forgotten() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    let (recording, association) = world.association().await;
    world
        .hito
        .artist_catalog
        .choose_album_edition(
            &world.actor,
            &world.release_id,
            ChooseAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                recording_id: recording,
                association_id: association,
                release_group_id: "owner-group".into(),
                edition_id: "owner-edition".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
    world
        .hito
        .artist_catalog
        .clear_album_edition(
            &world.actor,
            &world.release_id,
            ClearAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
    assert!(world.release_requests().is_empty());
}

#[tokio::test]
async fn unknown_ids_outages_missing_consent_and_missing_artists_fall_back() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world.fake.release(RELEASE, Reply::NotFound);
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
    assert_eq!(world.release_requests().len(), 1);
    world.fake.release(RELEASE, Reply::Unavailable);
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
    assert_eq!(
        world.release_requests().len(),
        2,
        "An outage is asked once and not retried"
    );

    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    world.consent(false).await;
    assert_eq!(world.lookup().await, TaggedAlbumEdition::None);
    assert_eq!(
        world.release_requests().len(),
        2,
        "Consent off asks nothing"
    );
    world.consent(true).await;
    found(world.lookup().await);

    let unmatched = World::prepare(same(RELEASE, None), false).await;
    unmatched
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    assert_eq!(unmatched.lookup().await, TaggedAlbumEdition::None);
    assert!(
        unmatched.release_requests().is_empty(),
        "Without a reviewed MusicBrainz artist nothing is asked"
    );
}

#[tokio::test]
async fn a_cancelled_lookup_returns_at_once_and_a_foreign_album_is_not_found() {
    let world = World::prepare(same(RELEASE, None), true).await;
    world
        .fake
        .release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    world.fake.delay_replies(Duration::from_secs(5));
    let started = std::time::Instant::now();
    let result = world
        .hito
        .artist_catalog
        .tagged_album_edition_until(
            &world.actor,
            &world.release_id,
            tokio::time::sleep(Duration::from_millis(200)),
        )
        .await
        .unwrap();
    assert_eq!(result, TaggedAlbumEdition::None);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(matches!(
        world
            .hito
            .artist_catalog
            .tagged_album_edition_until(&world.actor, "missing", std::future::pending())
            .await,
        Err(hito_core::Error::NotFound)
    ));
}

#[tokio::test]
async fn the_details_pages_show_only_the_identifiers_hito_holds() {
    let world = World::prepare(same(RELEASE, Some(TAGGED_GROUP)), true).await;
    let album = world
        .hito
        .catalog
        .release_metadata(&world.actor, &world.release_id, Default::default())
        .await
        .unwrap();
    assert_eq!(album.musicbrainz.release_id.as_deref(), Some(RELEASE));
    assert_eq!(
        album.musicbrainz.release_group_id.as_deref(),
        Some(TAGGED_GROUP)
    );
    let recording = world
        .hito
        .catalog
        .recording_metadata(&world.actor, &world.recordings[0], Default::default())
        .await
        .unwrap();
    assert!(
        recording
            .musicbrainz_recording_id
            .as_deref()
            .is_some_and(|id| id.starts_with("0f700000-0000-4000-8000-0000000000c"))
    );
    assert!(
        world
            .fake
            .requests()
            .iter()
            .all(|request| !request.contains("/release/"))
    );

    let mixed = World::prepare(
        vec![
            Track {
                release: Some(RELEASE),
                group: None,
            },
            Track {
                release: Some(OTHER_RELEASE),
                group: None,
            },
        ],
        false,
    )
    .await;
    let album = mixed
        .hito
        .catalog
        .release_metadata(&mixed.actor, &mixed.release_id, Default::default())
        .await
        .unwrap();
    assert_eq!(album.musicbrainz.release_id, None);
    assert_eq!(album.musicbrainz.release_group_id, None);
}
