use hito_core::test_support::support::{library, scan, write_wav};
use hito_core::{Credentials, Error};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use std::path::Path;

fn tag(path: &Path, offset: i16, (title, album, artist): (&str, &str, &str), genre: Option<&str>) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    if let Some(genre) = genre {
        tag.set_genre(genre.into());
    }
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[tokio::test]
async fn genres_group_tagged_library_tracks_and_open_into_albums_and_tracks() {
    let (_directory, hito, actor, library_id, music) = library("genres").await;
    tag(
        &music.join("1.wav"),
        1,
        ("Song 1", "Album X", "Ana"),
        Some("Fado"),
    );
    tag(
        &music.join("2.wav"),
        2,
        ("Song 2", "Album X", "Ana"),
        Some("fado"),
    );
    tag(
        &music.join("3.wav"),
        3,
        ("Song 3", "Album Y", "Bia"),
        Some("Fado; Jazz"),
    );
    tag(
        &music.join("4.wav"),
        4,
        ("Song 4", "Album Z", "Caio"),
        Some("Jazz"),
    );
    tag(&music.join("5.wav"), 5, ("Song 5", "Album W", "Dora"), None);
    for number in 0..25 {
        tag(
            &music.join(format!("g{number:02}.wav")),
            10 + number,
            (&format!("Solo {number}"), "Album S", "Eva"),
            Some(&format!("Solo genre {number:02}")),
        );
    }
    scan(&hito, &actor, &library_id).await;

    let genres = hito.catalog.genres(&actor).await.unwrap();
    assert_eq!(genres.len(), 18, "tiles are bounded");
    assert_eq!(
        genres[..2]
            .iter()
            .map(|genre| (genre.key.as_str(), genre.label.as_str(), genre.track_count))
            .collect::<Vec<_>>(),
        vec![("fado", "Fado", 3), ("jazz", "Jazz", 2)],
        "case variants and split values count once per track, busiest first"
    );
    assert!(genres.iter().all(|genre| genre.key != "album w"));

    let fado = hito.catalog.genre(&actor, "  FADO ").await.unwrap();
    assert_eq!(fado.genre.label, "Fado");
    assert_eq!(fado.genre.track_count, 3);
    let mut titles: Vec<_> = fado
        .tracks
        .iter()
        .map(|track| track.title.clone())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Song 1", "Song 2", "Song 3"]);
    assert_eq!(
        fado.albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        ["Album X", "Album Y"],
        "albums with more tracks in the genre come first"
    );
    assert!(matches!(
        hito.catalog.genre(&actor, "blues").await,
        Err(Error::NotFound)
    ));

    hito.identity
        .create_user(
            &actor,
            Credentials {
                username: "outsider".into(),
                password: "owned genre outsider password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "outsider".into(),
            password: "owned genre outsider password".into(),
        })
        .await
        .unwrap();
    let outsider = hito.identity.authenticate(&session.token).await.unwrap();
    assert!(hito.catalog.genres(&outsider).await.unwrap().is_empty());
    assert!(matches!(
        hito.catalog.genre(&outsider, "fado").await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn retagging_a_file_replaces_its_genres_after_the_next_scan() {
    let (_directory, hito, actor, library_id, music) = library("genres-retag").await;
    tag(
        &music.join("1.wav"),
        1,
        ("Song 1", "Album X", "Ana"),
        Some("Jazz"),
    );
    tag(
        &music.join("2.wav"),
        2,
        ("Song 2", "Album Y", "Bia"),
        Some("Jazz"),
    );
    scan(&hito, &actor, &library_id).await;
    assert_eq!(hito.catalog.genres(&actor).await.unwrap()[0].track_count, 2);

    tag(
        &music.join("2.wav"),
        2,
        ("Song 2", "Album Y", "Bia"),
        Some("Folk"),
    );
    scan(&hito, &actor, &library_id).await;

    let genres = hito.catalog.genres(&actor).await.unwrap();
    let counts: Vec<_> = genres
        .iter()
        .map(|g| (g.key.as_str(), g.track_count))
        .collect();
    assert_eq!(counts, [("folk", 1), ("jazz", 1)]);
}
