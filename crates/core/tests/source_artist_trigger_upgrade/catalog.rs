use hito_core::{Browse, Credentials, test_support::support};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn rescanned_source_artists_are_visible_only_in_the_owning_accounts_catalog() {
    let (directory, core, actor, library, music) = support::library("source-artist-host").await;
    let file = music.join("owned.wav");
    support::write_wav(&file, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Owned track".into());
    tag.set_artist("Owned source artist".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    support::scan(&core, &actor, &library).await;
    let original = core
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(original.total, 1);
    assert_eq!(original.items[0].name, "Owned source artist");
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let original_id: String = sqlx::query_scalar(
        "SELECT id FROM artists WHERE library_id=? AND name='Owned source artist'",
    )
    .bind(&library)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    tag.set_artist("Owned changed source".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    support::scan(&core, &actor, &library).await;
    let updated = core
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(updated.total, 1);
    assert_eq!(updated.items[0].name, "Owned changed source");
    let current_id: String = sqlx::query_scalar(
        "SELECT id FROM artists WHERE library_id=? AND name='Owned source artist'",
    )
    .bind(&library)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        current_id, original_id,
        "source artist updates must retain earlier legacy identities"
    );
    let changed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM artists WHERE library_id=? AND name='Owned changed source'",
    )
    .bind(&library)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(changed, 1);
    let credentials = || Credentials {
        username: "other-reader".into(),
        password: "owned other reader password".into(),
    };
    core.identity
        .create_user(&actor, credentials())
        .await
        .unwrap();
    let other = core.identity.login(credentials()).await.unwrap();
    let other = core.identity.authenticate(&other.token).await.unwrap();
    assert_eq!(
        core.catalog
            .artists(&other, Browse::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(
        core.catalog
            .artist(&other, &updated.items[0].id, Browse::default())
            .await
            .is_err()
    );
    connection.close().await.unwrap();
}
