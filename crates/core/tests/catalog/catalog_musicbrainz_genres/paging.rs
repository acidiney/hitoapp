use super::fixture::{A, GROUP, Ids, World, paged_tracks_match_preview, pairs, two_artists};
use hito_core::test_support::musicbrainz::Reply;
use std::{collections::HashSet, time::Duration};

#[tokio::test]
async fn genre_track_pages_deduplicate_tagged_sources_before_the_next_candidate() {
    let world = hito_core::test_support::hang_guard::hang_guard(
        "genre track duplicate tagged source setup",
        Box::pin(async {
            let world = two_artists().await;
            world.track(
                5,
                "first-rock",
                Ids {
                    artist: Some(A),
                    genre: Some("Rock"),
                    ..Ids::default()
                },
            );
            std::fs::copy(
                world.music.join("first-rock.wav"),
                world.music.join("first-rock-copy.wav"),
            )
            .unwrap();
            world.track(
                6,
                "second-rock",
                Ids {
                    genre: Some("Rock"),
                    ..Ids::default()
                },
            );
            world.scan().await;
            world.walk().await;
            world
        }),
    )
    .await;

    let tiles = tokio::time::timeout(Duration::from_secs(5), world.tiles())
        .await
        .expect("genre tiles finish within 5 seconds");
    assert_eq!(
        tiles,
        pairs(&[("fado", 2), ("folk", 2), ("jazz", 1), ("rock", 2)])
    );
    let requests = world.genre_requests();
    let fado = paged_tracks_match_preview(&world, "fado").await;
    assert_eq!(
        fado.iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        ["a1", "a2"]
    );
    assert!(fado.iter().all(|track| track.title != "first-rock"));
    let rock = paged_tracks_match_preview(&world, "rock").await;
    assert_eq!(rock.len(), 2);
    assert_eq!(
        rock.iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        ["first-rock", "second-rock"]
    );
    let duplicate = rock
        .iter()
        .find(|track| track.title == "first-rock")
        .unwrap();
    let sources = tokio::time::timeout(
        Duration::from_secs(5),
        world.hito.catalog.sources(&world.actor, &duplicate.id),
    )
    .await
    .expect("recording sources finish within 5 seconds")
    .unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| source.provider == "local"));
    assert_ne!(sources[0].id, sources[1].id);
    assert_eq!(world.genre_requests(), requests);
}

#[tokio::test]
async fn artist_and_release_group_genres_page_one_recording_once() {
    let world = hito_core::test_support::hang_guard::hang_guard(
        "genre track cross-identity setup",
        Box::pin(async {
            let world = World::new("mb-genre-cross-identity").await;
            world.track(
                1,
                "artist-and-group",
                Ids {
                    artist: Some(A),
                    group: Some(GROUP),
                    ..Ids::default()
                },
            );
            world.track(
                2,
                "artist-only",
                Ids {
                    artist: Some(A),
                    ..Ids::default()
                },
            );
            world.artist_genres(A, &[("fado", 1), ("folk", 1)]);
            world.fake.genres_of(
                "release-group",
                GROUP,
                Reply::Genres(vec![("fado".into(), 1), ("ambient".into(), 1)]),
            );
            world.scan().await;
            world.walk().await;
            world
        }),
    )
    .await;

    let requests = world.genre_requests();
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("/ws/2/artist/"))
    );
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("/ws/2/release-group/"))
    );
    let fado = paged_tracks_match_preview(&world, "fado").await;
    assert_eq!(fado.len(), 2);
    let fado_ids: HashSet<_> = fado.iter().map(|track| track.id.clone()).collect();
    let folk = paged_tracks_match_preview(&world, "folk").await;
    assert_eq!(folk.len(), 2);
    let folk_ids: HashSet<_> = folk.iter().map(|track| track.id.clone()).collect();
    assert_eq!(fado_ids, folk_ids);
    let grouped = fado
        .iter()
        .find(|track| track.title == "artist-and-group")
        .unwrap();
    let ambient = paged_tracks_match_preview(&world, "ambient").await;
    assert_eq!(ambient.len(), 1);
    assert_eq!(grouped.id, ambient[0].id);
    assert_eq!(world.genre_requests(), requests);
}
