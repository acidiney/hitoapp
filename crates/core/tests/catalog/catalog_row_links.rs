use hito_core::test_support::support;

use hito_core::{Action, Actor, Credentials, Search, Services};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use uuid::Uuid;

async fn tagged_library() -> (tempfile::TempDir, Services, Actor, String) {
    let (dir, hito, actor, library, music) = support::library("row-links").await;
    let file = music.join("track.wav");
    support::write_wav(&file, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Linked song".into());
    tag.set_artist("Linked performer".into());
    tag.set_album("Linked record".into());
    tag.save_to_path(file, Default::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    (dir, hito, actor, library)
}

async fn guest(hito: &Services, admin: &Actor, library: &str) -> Actor {
    let credentials = || Credentials {
        username: "guest".into(),
        password: "row links guest password".into(),
    };
    let created = hito
        .identity
        .create_user(admin, credentials())
        .await
        .unwrap();
    hito.identity
        .set_library_access(admin, library, &created.id, true)
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    hito.identity.authenticate(&session.token).await.unwrap()
}

async fn artist_named(hito: &Services, actor: &Actor, name: &str) -> Option<String> {
    hito.catalog
        .artists(actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|artist| artist.name == name)
        .map(|artist| artist.id)
}

async fn only_recording(hito: &Services, actor: &Actor) -> hito_core::Recording {
    let mut page = hito
        .catalog
        .recordings(actor, Search::default())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    page.items.remove(0)
}

#[tokio::test]
async fn recordings_carry_the_artist_id_the_artist_page_accepts() {
    let (_dir, hito, admin, _library) = tagged_library().await;
    let expected = artist_named(&hito, &admin, "Linked performer").await;
    assert!(expected.is_some());
    let listed = only_recording(&hito, &admin).await;
    assert_eq!(listed.artist_id, expected);
    let detail = hito
        .catalog
        .artist(&admin, expected.as_deref().unwrap(), Default::default())
        .await
        .unwrap();
    assert_eq!(detail.recordings.items[0].artist_id, expected);
    let release = listed.release_id.clone().unwrap();
    let album = hito
        .catalog
        .release(&admin, &release, Default::default())
        .await
        .unwrap();
    assert_eq!(album.tracks.items[0].recording.artist_id, expected);
}

#[tokio::test]
async fn another_account_never_sees_the_first_accounts_artist_choice() {
    let (_dir, hito, admin, library) = tagged_library().await;
    let other = guest(&hito, &admin, &library).await;
    let before = only_recording(&hito, &admin).await;
    let recording = Uuid::parse_str(&before.id).unwrap();
    support::synchronize(
        &hito,
        &admin,
        vec![Action::RecordingArtistSet {
            recording_id: recording,
            artist: "Private duo".into(),
        }],
    )
    .await;
    let mine = only_recording(&hito, &admin).await;
    let private = artist_named(&hito, &admin, "Private duo").await;
    assert!(private.is_some());
    assert_eq!(mine.artist_id, private);

    let theirs = only_recording(&hito, &other).await;
    assert_eq!(theirs.artist, "Linked performer");
    assert_eq!(
        theirs.artist_id,
        artist_named(&hito, &other, "Linked performer").await
    );
    assert!(theirs.artist_id.is_some());
    assert_ne!(theirs.artist_id, mine.artist_id);
    assert!(artist_named(&hito, &other, "Private duo").await.is_none());
}

#[tokio::test]
async fn playlist_entries_carry_the_album_title_and_release_id() {
    let (_dir, hito, admin, library) = tagged_library().await;
    let other = guest(&hito, &admin, &library).await;
    let track = only_recording(&hito, &admin).await;
    let playlist = Uuid::new_v4();
    support::synchronize(
        &hito,
        &admin,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Row links".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: Uuid::parse_str(&track.id).unwrap(),
                position: 0,
            },
        ],
    )
    .await;
    let detail = hito
        .collection
        .playlist(&admin, &playlist.to_string())
        .await
        .unwrap();
    let entry = &detail.entries[0];
    assert_eq!(entry.album.as_deref(), Some("Linked record"));
    assert_eq!(entry.release_id, track.release_id);
    assert!(entry.release_id.is_some());
    assert_eq!(
        entry.artist_id,
        artist_named(&hito, &admin, "Linked performer").await
    );
    assert!(
        hito.collection
            .playlist(&other, &playlist.to_string())
            .await
            .is_err()
    );
}
