use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support::{library, scan, synchronize, write_wav};
use hito_core::{Action, AlbumArtistInference, Credentials, ImportProviderPlaylist, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::Path;
use uuid::Uuid;

fn tag_wav(path: &Path, title: &str, album: &str, artist: &str) {
    tag_track(path, title, album, artist, None);
}

fn tag_track(path: &Path, title: &str, album: &str, artist: &str, number: Option<u32>) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    if let Some(number) = number {
        tag.set_track(number);
    }
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
    assert_eq!(home.recent_albums[0].artist.as_deref(), Some("Window"));
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

async fn guest_with_access(
    hito: &hito_core::Services,
    admin: &hito_core::Actor,
    library_id: &str,
) -> hito_core::Actor {
    let credentials = || Credentials {
        username: "guest".into(),
        password: "guest home test password".into(),
    };
    let guest = hito
        .identity
        .create_user(admin, credentials())
        .await
        .unwrap();
    hito.identity
        .set_library_access(admin, library_id, &guest.id, true)
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    hito.identity.authenticate(&session.token).await.unwrap()
}

#[tokio::test]
async fn unheard_tracks_follow_album_order_and_leave_when_their_source_goes() {
    let (_dir, hito, actor, library_id, music) = library("home-order").await;
    for (offset, (name, title, number)) in [
        ("o1.wav", "Zulu opener", 1),
        ("o2.wav", "Alpha closer", 2),
        ("o3.wav", "Mid song", 3),
        ("o4.wav", "Played first", 4),
    ]
    .into_iter()
    .enumerate()
    {
        let path = music.join(name);
        write_wav(&path, offset as i16);
        tag_track(&path, title, "Ordered", "Band", Some(number));
    }
    scan(&hito, &actor, &library_id).await;
    let all = tracks(&hito, &actor).await;
    let id = |title: &str| all.iter().find(|r| r.title == title).unwrap().id.clone();
    synchronize(&hito, &actor, vec![progress(&id("Played first"), 10)]).await;
    let home = hito.collection.home(&actor).await.unwrap();
    let titles: Vec<&str> = home.unheard[0]
        .tracks
        .iter()
        .map(|t| t.title.as_str())
        .collect();
    assert_eq!(titles, ["Zulu opener", "Alpha closer", "Mid song"]);
    assert_eq!(home.unheard[0].remaining, 3);

    std::fs::remove_file(music.join("o2.wav")).unwrap();
    scan(&hito, &actor, &library_id).await;
    let home = hito.collection.home(&actor).await.unwrap();
    let titles: Vec<&str> = home.unheard[0]
        .tracks
        .iter()
        .map(|t| t.title.as_str())
        .collect();
    assert_eq!(titles, ["Zulu opener", "Mid song"]);
    assert_eq!(home.unheard[0].remaining, 2);
}

#[tokio::test]
async fn removing_library_access_empties_the_home_summary() {
    let (_dir, hito, admin, library_id, music) = library("home-revoked").await;
    for (offset, title) in ["Shared one", "Shared two"].into_iter().enumerate() {
        let path = music.join(format!("{title}.wav"));
        write_wav(&path, offset as i16);
        tag_wav(&path, title, "Shared", "Owner");
    }
    scan(&hito, &admin, &library_id).await;
    let guest = guest_with_access(&hito, &admin, &library_id).await;
    let all = tracks(&hito, &guest).await;
    synchronize(
        &hito,
        &guest,
        vec![
            progress(&all[0].id, 500),
            Action::LikeSet {
                recording_id: Uuid::parse_str(&all[0].id).unwrap(),
                liked: true,
            },
        ],
    )
    .await;
    let home = hito.collection.home(&guest).await.unwrap();
    assert_eq!(home.liked_count, 1);
    assert_eq!(home.recent_albums.len(), 1);
    assert_eq!(home.unheard.len(), 1);
    hito.identity
        .set_library_access(&admin, &library_id, &guest.user().id, false)
        .await
        .unwrap();
    let home = hito.collection.home(&guest).await.unwrap();
    assert!(!home.has_music);
    assert_eq!(home.liked_count, 0);
    assert!(home.recent_albums.is_empty());
    assert!(home.unheard.is_empty());
}

fn provider_track(id: &str, performer: &str, album: &str, album_artist: Option<&str>) -> Value {
    let mut release = json!({"id": album, "title": album});
    if let Some(artist) = album_artist {
        release["artist"] = json!(artist);
    }
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": id, "title": id, "artist": performer, "duration_ms": 100000,
        "release": release, "disc_number": 1, "track_number": 1, "revision": "v1"}}})
}

#[tokio::test]
async fn home_albums_without_an_album_artist_carry_the_inferred_credit() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _library, _music) = library("home-inferred").await;
    let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/home-inferred",
        "v1",
        vec![
            provider_track("a1", "Same Band", "Shared", None),
            provider_track("a2", "SAME BAND", "Shared", None),
            provider_track("b1", "First Band", "Mixed", None),
            provider_track("b2", "Second Band", "Mixed", None),
            provider_track("c1", "Credited Band", "Credited", Some("Album Owner")),
        ],
    )
    .await;
    hito.collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/home-inferred".into(),
            },
        )
        .await
        .unwrap();
    let all = tracks(&hito, &actor).await;
    let id = |title: &str| all.iter().find(|r| r.title == title).unwrap().id.clone();
    synchronize(
        &hito,
        &actor,
        vec![
            progress(&id("a1"), 1000),
            progress(&id("b1"), 1000),
            progress(&id("c1"), 1000),
        ],
    )
    .await;

    let home = hito.collection.home(&actor).await.unwrap();
    let album = |title: &str| {
        home.recent_albums
            .iter()
            .find(|album| album.title == title)
            .unwrap()
    };
    let shared = album("Shared");
    assert!(shared.artist.is_none());
    assert!(matches!(
        &shared.inferred_artist,
        Some(AlbumArtistInference::Named { name }) if name.eq_ignore_ascii_case("same band")
    ));
    assert!(matches!(
        album("Mixed").inferred_artist,
        Some(AlbumArtistInference::Various)
    ));
    let credited = album("Credited");
    assert_eq!(credited.artist.as_deref(), Some("Album Owner"));
    assert!(credited.inferred_artist.is_none());
}
