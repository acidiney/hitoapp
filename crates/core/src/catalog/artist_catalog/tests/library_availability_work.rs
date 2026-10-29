use crate::{
    ArtistCreditRole, AssociateRecordingArtist, Browse, ChooseAlbumEdition, MetadataProvider,
    ReadDiscographyLibrary, Search, UpdateMetadataSettings,
    test_support::{support, work_meter::WorkMeter},
};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use uuid::Uuid;

const TRACKS: i16 = 12;
const SMALL_CHOICES: i64 = 100;
const LARGE_CHOICES: i64 = 3_000;
const GROUPS: usize = 50;
const FIXED_WORK: u64 = 50_000;
const WORK_PER_CHOICE: u64 = 60;
const REAL_GROUP: &str = "real-release-group";

async fn seed(
    pool: &sqlx::SqlitePool,
    user: &str,
    recording: &str,
    association: &str,
    from: i64,
    to: i64,
) {
    sqlx::query(
        "WITH RECURSIVE n(value) AS (SELECT ? UNION ALL SELECT value + 1 FROM n WHERE value < ?)
        INSERT INTO album_edition_choices(user_id,release_id,revision,recording_id,association_id,provider_kind,release_group_id,edition_id)
        SELECT ?, 'synthetic-release-' || value, 1, ?, ?, 'builtin_musicbrainz', 'synthetic-group-' || (value % 500), 'edition-' || value FROM n",
    )
    .bind(from)
    .bind(to)
    .bind(user)
    .bind(recording)
    .bind(association)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn availability_work_grows_only_with_the_account_s_saved_choices() {
    let (_directory, hito, actor, library, music) =
        support::library("library-availability-work").await;
    for offset in 0..TRACKS {
        let path = music.join(format!("{offset}.wav"));
        support::write_wav(&path, offset + 1);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("Song {offset}"));
        tag.set_artist("Performer".into());
        tag.set_album("Real album".into());
        tag.save_to_path(&path, Default::default()).unwrap();
    }
    support::scan(&hito, &actor, &library).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    let recording = recordings.items[0].id.clone();
    let release = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap()
        .items[0]
        .id
        .clone();
    let artist = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap()
        .items[0]
        .id
        .clone();
    let observation = hito
        .catalog
        .recording_metadata(&actor, &recording, Default::default())
        .await
        .unwrap()
        .selected_observation
        .unwrap();
    let settings = hito.catalog.metadata_settings(&actor).await.unwrap();
    let settings = hito
        .catalog
        .set_metadata_settings(
            &actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![MetadataProvider::BuiltinMusicBrainz],
            },
        )
        .await
        .unwrap();
    let association = hito
        .catalog
        .associate_recording_artist(
            &actor,
            &recording,
            AssociateRecordingArtist {
                artist_id: artist,
                source_id: observation.source_id,
                observation_id: observation.id,
                role: ArtistCreditRole::RecordingArtist,
                provider: MetadataProvider::BuiltinMusicBrainz,
                provider_artist_id: Uuid::new_v4().to_string(),
                policy_revision: settings.revision,
            },
        )
        .await
        .unwrap();
    hito.catalog
        .choose_album_edition(
            &actor,
            &release,
            ChooseAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                recording_id: recording.clone(),
                association_id: association.id.clone(),
                release_group_id: REAL_GROUP.into(),
                edition_id: "real-edition".into(),
            },
        )
        .await
        .unwrap();
    let request = || ReadDiscographyLibrary {
        release_group_ids: std::iter::once(REAL_GROUP.to_owned())
            .chain((0..GROUPS - 1).map(|index| format!("synthetic-group-{index}")))
            .collect(),
    };
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let mut measured = Vec::new();
    for (from, to) in [(1, SMALL_CHOICES), (SMALL_CHOICES + 1, LARGE_CHOICES)] {
        seed(
            &hito.catalog.store.pool,
            actor.id(),
            &recording,
            &association.id,
            from,
            to,
        )
        .await;
        let (result, work) = meter
            .measure(hito.catalog.discography_library(
                &actor,
                &recording,
                &association.id,
                request(),
            ))
            .await;
        let result = result.unwrap();
        assert_eq!(result.groups.len(), GROUPS);
        let [album] = result.groups[0].albums.as_slice() else {
            panic!("The real group stays linked to its album");
        };
        assert_eq!(
            (album.track_count, album.playable_count),
            (TRACKS as i64, TRACKS as i64)
        );
        assert!(
            result.groups[1..]
                .iter()
                .all(|group| group.albums.is_empty()),
            "Saved choices for albums that are not in the library never link"
        );
        measured.push(work);
    }
    meter.remove(&hito.catalog.store.pool).await;
    eprintln!(
        "Library availability VM_steps small={} large={}",
        measured[0], measured[1]
    );
    let budget =
        measured[0] + FIXED_WORK + (LARGE_CHOICES - SMALL_CHOICES) as u64 * WORK_PER_CHOICE;
    assert!(
        measured[1] < budget,
        "Availability used {} VM steps with {LARGE_CHOICES} saved choices after {} with {SMALL_CHOICES}; budget {budget}",
        measured[1],
        measured[0]
    );
}

const WIDE_GROUPS: i16 = 50;
const ALBUMS_PER_GROUP: i16 = 3;
const WIDE_BUDGET: u64 = 600_000;

#[tokio::test]
async fn fifty_groups_with_three_albums_on_separate_associations_stay_in_budget() {
    use super::fixture::{associate, choose, enable_builtin, recording_of, write_album};
    let (_directory, hito, actor, library, music) =
        support::library("library-availability-wide").await;
    for index in 0..WIDE_GROUPS * ALBUMS_PER_GROUP {
        write_album(&music, &format!("Album {index}"), index, 1);
    }
    support::scan(&hito, &actor, &library).await;
    let revision = enable_builtin(&hito, &actor).await;
    let mut first = None;
    for index in 0..WIDE_GROUPS * ALBUMS_PER_GROUP {
        let (recording, release) = recording_of(&hito, &actor, &format!("Album {index}")).await;
        let association = associate(&hito, &actor, &recording, revision).await;
        choose(
            &hito,
            &actor,
            &release,
            &recording,
            &association.id,
            &format!("group-{}", index % WIDE_GROUPS),
        )
        .await;
        first.get_or_insert((recording, association.id));
    }
    let (recording, association) = first.unwrap();
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let (result, work) = meter
        .measure(
            hito.catalog.discography_library(
                &actor,
                &recording,
                &association,
                ReadDiscographyLibrary {
                    release_group_ids: (0..WIDE_GROUPS)
                        .map(|index| format!("group-{index}"))
                        .collect(),
                },
            ),
        )
        .await;
    meter.remove(&hito.catalog.store.pool).await;
    let result = result.unwrap();
    eprintln!("Library availability wide VM_steps={work}");
    assert!(
        result
            .groups
            .iter()
            .all(|group| group.albums.len() == ALBUMS_PER_GROUP as usize)
    );
    assert!(
        work < WIDE_BUDGET,
        "Wide availability used {work} VM steps against {WIDE_BUDGET}"
    );
}
