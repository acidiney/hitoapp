use self::fixture::{
    A, B, GROUP, Ids, RECORDING, VARIOUS_ARTISTS, World, paged_tracks_match_preview, pairs,
    two_artists,
};
use hito_core::test_support::{metadata_tag_logs::CapturedLogs, musicbrainz::Reply};
use hito_core::{Credentials, UpdateAutomaticLookups};
use std::time::Duration;
use tracing::instrument::WithSubscriber;

mod fixture;
mod paging;

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
    let genre_requests = world.genre_requests();
    let paged = paged_tracks_match_preview(&world, "fado").await;
    assert_eq!(paged.len(), 2);
    assert_eq!(world.genre_requests(), genre_requests);

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
    let outsider_page = tokio::time::timeout(
        Duration::from_secs(5),
        world
            .hito
            .catalog
            .genre_tracks(&outsider, "fado", Some(1), None),
    )
    .await
    .expect("outsider genre lookup finishes within 5 seconds");
    assert!(matches!(outsider_page, Err(hito_core::Error::NotFound)));
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
    let genre_requests = world.genre_requests();
    let fado_tracks = paged_tracks_match_preview(&world, "fado").await;
    assert_eq!(fado_tracks.len(), 2);
    assert!(fado_tracks.iter().all(|track| track.title != "tagged"));
    let rock_tracks = paged_tracks_match_preview(&world, "rock").await;
    assert_eq!(rock_tracks.len(), 1);
    assert_eq!(rock_tracks[0].title, "tagged");
    assert_eq!(world.genre_requests(), genre_requests);
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
    let genre_requests = world.genre_requests();
    let ambient = paged_tracks_match_preview(&world, "ambient").await;
    assert_eq!(ambient.len(), 1);
    assert_eq!(ambient[0].title, "grouped");
    let drone = paged_tracks_match_preview(&world, "drone").await;
    assert_eq!(drone.len(), 1);
    assert_eq!(drone[0].title, "recorded");
    assert_eq!(world.genre_requests(), genre_requests);
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

#[tokio::test]
async fn an_invalid_answer_is_cached_as_empty_and_retried_after_a_week_without_requeueing_walks() {
    let world = World::new("mb-genres-invalid").await;
    world.track(
        1,
        "a1",
        Ids {
            artist: Some(A),
            ..Ids::default()
        },
    );
    let too_many: Vec<(String, u32)> = (0..65).map(|index| (format!("g{index}"), 1)).collect();
    world.fake.genres_of("artist", A, Reply::Genres(too_many));
    world.scan().await;
    world.walk().await;

    assert_eq!(world.genre_requests().len(), 1);
    assert_eq!(
        world
            .count("SELECT count(*) FROM musicbrainz_genre_lookups")
            .await,
        1,
        "the invalid answer is remembered"
    );
    assert_eq!(
        world.count("SELECT count(*) FROM musicbrainz_genres").await,
        0
    );
    assert_eq!(
        world
            .count("SELECT count(*) FROM artist_matching_jobs")
            .await,
        0,
        "no retry walk is kept for it"
    );
    assert_eq!(
        world
            .hito
            .artist_catalog
            .queue_first_automatic_matching()
            .await
            .unwrap(),
        0,
        "a restart does not walk again"
    );

    world
        .execute("UPDATE musicbrainz_genre_lookups SET fetched_at = fetched_at - 8 * 86400")
        .await;
    assert_eq!(
        world
            .hito
            .artist_catalog
            .queue_first_automatic_matching()
            .await
            .unwrap(),
        1,
        "after a week it is asked again"
    );
}

#[tokio::test]
async fn other_accounts_genre_keys_cannot_fill_the_key_walk() {
    let world = two_artists().await;
    world.walk().await;
    world
        .execute(
            "INSERT INTO musicbrainz_genre_lookups(entity, mbid, fetched_at)
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2100)
             SELECT 'artist', printf('00000000-0000-4000-8000-%012d', i), unixepoch() FROM n",
        )
        .await;
    world
        .execute(
            "INSERT INTO musicbrainz_genres(entity, mbid, genre_key, label)
             SELECT 'artist', mbid, printf('aaa%05d', rowid), printf('Foreign %d', rowid)
             FROM musicbrainz_genre_lookups WHERE mbid LIKE '00000000-%'",
        )
        .await;
    assert_eq!(
        world.tiles().await,
        pairs(&[("fado", 2), ("folk", 2), ("jazz", 1)])
    );
}
