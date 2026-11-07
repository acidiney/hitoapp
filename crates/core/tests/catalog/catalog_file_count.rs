use hito_core::test_support::support;

use hito_core::{Credentials, Search};
use support::{library, scan, write_wav};

#[tokio::test]
async fn files_are_counted_separately_from_tracks_when_copies_share_one_recording() {
    let (_dir, hito, actor, library, music) = library("server").await;
    let original = music.join("original.wav");
    write_wav(&original, 0);
    std::fs::copy(&original, music.join("copy.wav")).unwrap();
    write_wav(&music.join("other.wav"), 1);
    scan(&hito, &actor, &library).await;

    let tracks = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    let files = hito.catalog.local_file_count(&actor).await.unwrap();

    assert_eq!(tracks.total, 2);
    assert_eq!(files, 3);
}

#[tokio::test]
async fn files_of_libraries_the_account_cannot_read_are_not_counted() {
    let (_dir, hito, actor, library, music) = library("server").await;
    write_wav(&music.join("one.wav"), 0);
    scan(&hito, &actor, &library).await;
    hito.identity
        .create_user(
            &actor,
            Credentials {
                username: "stranger".into(),
                password: "stranger password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "stranger".into(),
            password: "stranger password".into(),
        })
        .await
        .unwrap();
    let stranger = hito.identity.authenticate(&session.token).await.unwrap();

    assert_eq!(hito.catalog.local_file_count(&actor).await.unwrap(), 1);
    assert_eq!(hito.catalog.local_file_count(&stranger).await.unwrap(), 0);
}
