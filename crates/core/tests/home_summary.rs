use hito_core::test_support::support::{library, scan, synchronize, write_wav};
use hito_core::{Action, Credentials, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::Path;
use uuid::Uuid;

fn tag_wav(path: &Path, title: &str, album: &str, artist: &str) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    tag.insert_text(ItemKey::AlbumArtist, artist.into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

async fn tracks(hito: &hito_core::Services, actor: &hito_core::Actor) -> Vec<hito_core::Recording> {
    hito.catalog
        .recordings(actor, Search::default())
        .await
        .unwrap()
        .items
}

fn progress(recording: &str, position_ms: i64) -> Action {
    Action::ProgressSet {
        recording_id: Uuid::parse_str(recording).unwrap(),
        position_ms,
        speed: 1.0,
    }
}

async fn set_played_at(directory: &Path, recording: &str, at: i64) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(directory.join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE listening_progress SET updated_at = ? WHERE recording_id = ?")
        .bind(at)
        .bind(recording)
        .execute(&mut connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_new_account_with_no_music_has_an_empty_home() {
    let (_dir, hito, actor, _library, _music) = library("home-empty").await;
    let home = hito.collection.home(&actor).await.unwrap();
    assert!(!home.has_music);
    assert_eq!(home.liked_count, 0);
    assert!(home.recent_albums.is_empty());
    assert!(home.unheard.is_empty());
}

#[tokio::test]
async fn home_lists_recent_albums_and_unheard_tracks_from_albums_the_account_played() {
    let (dir, hito, actor, library_id, music) = library("home-albums").await;
    for (offset, (name, title, album, artist)) in [
        ("a1.wav", "Tide one", "Tide", "Shore"),
        ("a2.wav", "Tide two", "Tide", "Shore"),
        ("a3.wav", "Tide three", "Tide", "Shore"),
        ("b1.wav", "Glass one", "Glass", "Window"),
        ("c1.wav", "Never one", "Never", "Quiet"),
        ("c2.wav", "Never two", "Never", "Quiet"),
    ]
    .into_iter()
    .enumerate()
    {
        let path = music.join(name);
        write_wav(&path, offset as i16);
        tag_wav(&path, title, album, artist);
    }
    scan(&hito, &actor, &library_id).await;
    let all = tracks(&hito, &actor).await;
    let id = |title: &str| all.iter().find(|r| r.title == title).unwrap().id.clone();
    synchronize(
        &hito,
        &actor,
        vec![
            progress(&id("Tide one"), 1000),
            progress(&id("Glass one"), 1000),
            Action::LikeSet {
                recording_id: Uuid::parse_str(&id("Never one")).unwrap(),
                liked: true,
            },
        ],
    )
    .await;
    set_played_at(dir.path(), &id("Tide one"), 100).await;
    set_played_at(dir.path(), &id("Glass one"), 200).await;

    let home = hito.collection.home(&actor).await.unwrap();
    assert!(home.has_music);
    assert_eq!(home.liked_count, 1);
    let titles: Vec<&str> = home
        .recent_albums
        .iter()
        .map(|album| album.title.as_str())
        .collect();
    assert_eq!(titles, ["Glass", "Tide"]);
    assert_eq!(home.recent_albums[0].artist, "Window");
    let tide = home.unheard.iter().find(|a| a.title == "Tide").unwrap();
    assert_eq!(tide.played, 1);
    let mut unheard: Vec<&str> = tide.tracks.iter().map(|t| t.title.as_str()).collect();
    unheard.sort();
    assert_eq!(unheard, ["Tide three", "Tide two"]);
    assert!(tide.tracks.iter().all(|track| track.available));
    assert!(
        home.unheard.iter().all(|album| album.title != "Glass"),
        "an album with nothing left to hear is not offered"
    );
    assert!(
        home.unheard.iter().all(|album| album.title != "Never"),
        "only albums the account has played are offered"
    );
}

#[tokio::test]
async fn home_never_reveals_another_accounts_listening() {
    let (_dir, hito, admin, library_id, music) = library("home-private").await;
    write_wav(&music.join("p1.wav"), 1);
    write_wav(&music.join("p2.wav"), 2);
    tag_wav(&music.join("p1.wav"), "Private one", "Private", "Owner");
    tag_wav(&music.join("p2.wav"), "Private two", "Private", "Owner");
    scan(&hito, &admin, &library_id).await;
    let all = tracks(&hito, &admin).await;
    synchronize(
        &hito,
        &admin,
        vec![
            progress(&all[0].id, 500),
            Action::LikeSet {
                recording_id: Uuid::parse_str(&all[0].id).unwrap(),
                liked: true,
            },
        ],
    )
    .await;
    let credentials = || Credentials {
        username: "guest".into(),
        password: "guest home test password".into(),
    };
    let guest = hito
        .identity
        .create_user(&admin, credentials())
        .await
        .unwrap();
    hito.identity
        .set_library_access(&admin, &library_id, &guest.id, true)
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    let guest = hito.identity.authenticate(&session.token).await.unwrap();
    let home = hito.collection.home(&guest).await.unwrap();
    assert!(home.has_music);
    assert_eq!(home.liked_count, 0);
    assert!(home.recent_albums.is_empty());
    assert!(home.unheard.is_empty());
}
