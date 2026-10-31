use hito_core::test_support::{
    metadata_tag_logs::CapturedLogs,
    musicbrainz::{FakeMusicBrainz, Reply},
    support,
};
use hito_core::{Actor, Credentials, Genre, Services, UpdateAutomaticLookups};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use sqlx::Connection;
use std::{path::PathBuf, time::Duration};
use tracing::instrument::WithSubscriber;

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const GROUP: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const RECORDING: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

#[derive(Default)]
struct Ids<'a> {
    artist: Option<&'a str>,
    group: Option<&'a str>,
    genre: Option<&'a str>,
}

struct World {
    hito: Services,
    actor: Actor,
    music: PathBuf,
    library: String,
    fake: FakeMusicBrainz,
    directory: tempfile::TempDir,
}

impl World {
    async fn new(host: &str) -> Self {
        let fake = FakeMusicBrainz::start().await;
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz(host, Some(fake.endpoint.clone())).await;
        Self {
            hito,
            actor,
            music,
            library,
            fake,
            directory,
        }
    }

    fn track(&self, offset: i16, title: &str, ids: Ids) {
        let path = self.music.join(format!("{title}.wav"));
        support::write_wav(&path, offset);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.into());
        tag.set_artist("Performer".into());
        tag.set_album(format!("Album {title}"));
        if let Some(genre) = ids.genre {
            tag.set_genre(genre.into());
        }
        for (key, value) in [
            (ItemKey::MusicBrainzArtistId, ids.artist),
            (ItemKey::MusicBrainzReleaseGroupId, ids.group),
        ] {
            if let Some(value) = value {
                tag.insert_text(key, value.into());
            }
        }
        tag.save_to_path(&path, Default::default()).unwrap();
    }

    /// A FLAC file, whose Vorbis comment names the recording; an ID3 track id is ambiguous.
    fn flac_track(&self, offset: i16, title: &str, recording: &str) {
        let source = self.directory.path().join(format!("{title}.wav"));
        support::write_wav(&source, offset);
        let path = self.music.join(format!("{title}.flac"));
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

    fn artist_genres(&self, artist: &str, genres: &[(&str, u32)]) {
        self.fake.genres_of(
            "artist",
            artist,
            Reply::Genres(
                genres
                    .iter()
                    .map(|(name, count)| ((*name).to_owned(), *count))
                    .collect(),
            ),
        );
    }

    async fn scan(&self) {
        support::scan(&self.hito, &self.actor, &self.library).await;
    }

    async fn walk(&self) -> usize {
        let mut steps = 0;
        while self
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
        {
            steps += 1;
            assert!(steps < 60, "The automatic walk must end");
        }
        steps
    }

    fn genre_requests(&self) -> Vec<String> {
        self.fake
            .requests()
            .into_iter()
            .filter(|request| request.contains("inc=genres"))
            .collect()
    }

    async fn tiles(&self) -> Vec<(String, i64)> {
        tiles(self.hito.catalog.genres(&self.actor).await.unwrap())
    }

    async fn connect(&self) -> sqlx::SqliteConnection {
        sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}",
            self.directory.path().join("hito.db").display()
        ))
        .await
        .unwrap()
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&mut self.connect().await)
            .await
            .unwrap()
    }

    async fn execute(&self, sql: &str) {
        sqlx::query(sql)
            .execute(&mut self.connect().await)
            .await
            .unwrap();
    }
}

fn tiles(genres: Vec<Genre>) -> Vec<(String, i64)> {
    genres
        .into_iter()
        .map(|genre| (genre.key, genre.track_count))
        .collect()
}

fn pairs(expected: &[(&str, i64)]) -> Vec<(String, i64)> {
    expected
        .iter()
        .map(|(key, count)| ((*key).to_owned(), *count))
        .collect()
}

async fn two_artists() -> World {
    let world = World::new("mb-genres").await;
    for (offset, title, artist) in [(1, "a1", A), (2, "a2", A), (3, "b1", B)] {
        world.track(
            offset,
            title,
            Ids {
                artist: Some(artist),
                ..Ids::default()
            },
        );
    }
    world.track(4, "plain", Ids::default());
    world.artist_genres(A, &[("fado", 5), ("folk", 1)]);
    world.artist_genres(B, &[("jazz", 3)]);
    world.scan().await;
    world
}

#[tokio::test]
async fn artist_genres_fill_tiles_and_pages_for_tracks_without_a_genre_tag() {
    let world = two_artists().await;
    assert!(
        world.tiles().await.is_empty(),
        "nothing is known before the walk"
    );
    world.walk().await;

    assert_eq!(
        world.tiles().await,
        pairs(&[("fado", 2), ("folk", 2), ("jazz", 1)])
    );
    let page = world
        .hito
        .catalog
        .genre(&world.actor, "fado")
        .await
        .unwrap();
    let mut titles: Vec<_> = page
        .tracks
        .iter()
        .map(|track| track.title.clone())
        .collect();
    titles.sort();
    assert_eq!(titles, ["a1", "a2"]);
    assert_eq!(page.albums.len(), 2);
    assert_eq!(world.genre_requests().len(), 2, "one request per artist");
    assert!(
        world
            .genre_requests()
            .iter()
            .all(|request| request.starts_with("/ws/2/artist/")),
        "{:?}",
        world.genre_requests()
    );

    assert_eq!(world.walk().await, 0, "a finished walk leaves no work");
    assert_eq!(
        world
            .hito
            .artist_catalog
            .queue_first_automatic_matching()
            .await
            .unwrap(),
        0,
        "fresh lookups queue nothing at the next start"
    );
    assert_eq!(world.genre_requests().len(), 2, "the cache spares repeats");

    let outsider = world
        .hito
        .identity
        .create_user(
            &world.actor,
            Credentials {
                username: "outsider".into(),
                password: "owned genre outsider password".into(),
            },
        )
        .await
        .unwrap();
    let session = world
        .hito
        .identity
        .login(Credentials {
            username: outsider.username.clone(),
            password: "owned genre outsider password".into(),
        })
        .await
        .unwrap();
    let outsider = world
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(
        world
            .hito
            .catalog
            .genres(&outsider)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn file_tag_genres_win_for_their_recording_and_other_tracks_still_use_musicbrainz() {
    let world = two_artists().await;
    world.track(
        5,
        "tagged",
        Ids {
            artist: Some(A),
            genre: Some("Rock"),
            ..Ids::default()
        },
    );
    world.scan().await;
    world.walk().await;

    assert_eq!(
        world.tiles().await,
        pairs(&[("fado", 2), ("folk", 2), ("jazz", 1), ("rock", 1)])
    );
    let fado = world
        .hito
        .catalog
        .genre(&world.actor, "fado")
        .await
        .unwrap();
    assert!(fado.tracks.iter().all(|track| track.title != "tagged"));
    let rock = world
        .hito
        .catalog
        .genre(&world.actor, "rock")
        .await
        .unwrap();
    assert_eq!(rock.tracks.len(), 1);
}

#[tokio::test]
async fn release_group_and_recording_genres_cover_tracks_with_no_artist_identity() {
    let world = World::new("mb-genres-other").await;
    world.track(
        1,
        "grouped",
        Ids {
            group: Some(GROUP),
            ..Ids::default()
        },
    );
    world.flac_track(2, "recorded", RECORDING);
    world.fake.genres_of(
        "release-group",
        GROUP,
        Reply::Genres(vec![("ambient".into(), 4)]),
    );
    world.fake.genres_of(
        "recording",
        RECORDING,
        Reply::Genres(vec![("drone".into(), 2)]),
    );
    world.scan().await;
    world.walk().await;

    assert_eq!(world.tiles().await, pairs(&[("ambient", 1), ("drone", 1)]));
    let requests = world.genre_requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("/ws/2/release-group/"))
    );
    assert!(requests.iter().any(|r| r.starts_with("/ws/2/recording/")));
}

#[tokio::test]
async fn an_outage_caches_nothing_and_a_later_walk_fills_the_genres_in() {
    let world = two_artists().await;
    world.fake.genres_of("artist", A, Reply::Unavailable);
    world.walk().await;

    assert_eq!(world.tiles().await, pairs(&[("jazz", 1)]));
    assert_eq!(
        world
            .count("SELECT count(*) FROM musicbrainz_genre_lookups")
            .await,
        1,
        "only the artist that answered is cached"
    );
    assert_eq!(
        world
            .count("SELECT count(*) FROM artist_matching_jobs")
            .await,
        1,
        "the walk is kept to be retried"
    );

    world.artist_genres(A, &[("fado", 5)]);
    world
        .execute("UPDATE artist_matching_jobs SET next_run_at = 0")
        .await;
    world.walk().await;
    assert_eq!(world.tiles().await, pairs(&[("fado", 2), ("jazz", 1)]));
    assert_eq!(
        world
            .count("SELECT count(*) FROM artist_matching_jobs")
            .await,
        0
    );
}

#[tokio::test]
async fn turning_automatic_lookups_off_sends_no_genre_request() {
    let world = two_artists().await;
    world
        .hito
        .artist_catalog
        .set_automatic_lookups(
            &world.actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: Some(false),
                automatic_artwork: None,
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    world.walk().await;
    assert!(world.fake.requests().is_empty());
    assert!(world.tiles().await.is_empty());
}

#[tokio::test]
async fn special_purpose_artists_are_never_looked_up() {
    let world = World::new("mb-genres-special").await;
    world.track(
        1,
        "compilation",
        Ids {
            artist: Some(VARIOUS_ARTISTS),
            ..Ids::default()
        },
    );
    world.artist_genres(VARIOUS_ARTISTS, &[("pop", 9)]);
    world.scan().await;
    world.walk().await;
    assert!(world.genre_requests().is_empty());
    assert!(world.tiles().await.is_empty());
}

#[tokio::test]
async fn lookups_older_than_thirty_days_are_asked_again_and_replace_their_genres() {
    let world = two_artists().await;
    world.walk().await;
    assert_eq!(world.genre_requests().len(), 2);

    world
        .execute("UPDATE musicbrainz_genre_lookups SET fetched_at = unixepoch() - 29 * 86400")
        .await;
    assert_eq!(
        world
            .hito
            .artist_catalog
            .queue_first_automatic_matching()
            .await
            .unwrap(),
        0
    );

    world
        .execute(&format!(
            "UPDATE musicbrainz_genre_lookups SET fetched_at = unixepoch() - 31 * 86400 WHERE mbid = '{A}'"
        ))
        .await;
    world.artist_genres(A, &[("fado", 2), ("modal", 1)]);
    assert_eq!(
        world
            .hito
            .artist_catalog
            .queue_first_automatic_matching()
            .await
            .unwrap(),
        1,
        "a finished account with a stale lookup is walked again"
    );
    world.walk().await;
    assert_eq!(
        world.genre_requests().len(),
        3,
        "only the stale artist is asked"
    );
    assert_eq!(
        world.tiles().await,
        pairs(&[("fado", 2), ("modal", 2), ("jazz", 1)])
    );
}

#[tokio::test]
async fn a_walk_asks_in_bounded_batches_resumes_from_its_cursor_and_stops_on_shutdown() {
    let world = World::new("mb-genres-batches").await;
    let artists: Vec<String> = (1..=10)
        .map(|index| format!("eeeeeeee-eeee-4eee-8eee-{index:012x}"))
        .collect();
    for (index, artist) in artists.iter().enumerate() {
        world.track(
            index as i16 + 1,
            &format!("t{index}"),
            Ids {
                artist: Some(artist),
                ..Ids::default()
            },
        );
        world.artist_genres(artist, &[("fado", 1)]);
    }
    world.scan().await;
    let mut resumed_from_cursor = false;
    for _ in 0..40 {
        if !world
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
        {
            break;
        }
        let cursor: Option<String> = sqlx::query_scalar("SELECT cursor FROM artist_matching_jobs")
            .fetch_optional(&mut world.connect().await)
            .await
            .unwrap()
            .flatten();
        if cursor.is_some_and(|cursor| cursor.starts_with("genres:")) {
            resumed_from_cursor = true;
            world.fake.delay_replies(Duration::from_secs(30));
            let started = std::time::Instant::now();
            world
                .hito
                .artist_catalog
                .run_next_automatic_matching_until(tokio::time::sleep(Duration::from_millis(300)))
                .await
                .unwrap();
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(
                world
                    .count("SELECT count(*) FROM artist_matching_jobs WHERE lease_token IS NULL")
                    .await,
                1,
                "the interrupted job is free to resume"
            );
            world.fake.delay_replies(Duration::ZERO);
        }
    }
    assert!(
        resumed_from_cursor,
        "a batch ended before every artist was asked"
    );
    assert_eq!(world.tiles().await, pairs(&[("fado", 10)]));
    let requests = world.genre_requests();
    let mut distinct = requests.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 10, "{requests:?}");
}

#[tokio::test]
async fn the_genre_pass_logs_counts_without_identifiers_names_or_titles() {
    let world = two_artists().await;
    let logs = CapturedLogs::default();
    let dispatch = tracing::Dispatch::new(logs.subscriber());
    world.walk().with_subscriber(dispatch).await;
    let text = logs.text();
    assert!(
        text.contains("MusicBrainz genre lookups finished"),
        "{text}"
    );
    for private in [A, B, "fado", "folk", "jazz", "Performer"] {
        assert!(!text.contains(private), "logs must not carry {private}");
    }
}
