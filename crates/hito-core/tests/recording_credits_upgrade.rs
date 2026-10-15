#[path = "fixtures/legacy_library.rs"]
mod legacy;
mod support;

use hito_core::Browse;
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;

#[tokio::test]
async fn upgrade_exposes_existing_albumless_recordings_and_preserves_private_credit_ids() {
    let fixture = legacy::Legacy::seed().await;
    let pool = sqlx::SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 20)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO libraries(id,name,owner_id) VALUES ('personal','Prior provider recordings','legacy-admin')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO library_members VALUES ('personal','legacy-admin')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO recordings(id,library_id,title,artist,provenance) VALUES ('prior-track','personal','Saved without sources','Observed artist','provider_response'),('private-track','personal','Private recording','Other observed artist','provider_response')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO personal_recording_metadata(user_id,recording_id,override_artist,title,artist,artist_operation_id) VALUES ('legacy-admin','private-track','Personal artist','Private recording','Personal artist','prior-artist-operation')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO account_artists(user_id,id,library_id,name) VALUES ('legacy-admin','stable-private-credit','personal','Personal artist')").execute(&pool).await.unwrap();
    pool.close().await;
    let (hito, actor) = fixture.open().await;
    let page = hito
        .catalog
        .artists(
            &actor,
            Browse {
                library: Some("personal".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    for artist in page.items {
        let detail = hito
            .catalog
            .artist(&actor, &artist.id, Default::default())
            .await
            .unwrap();
        assert_eq!(detail.releases.total, 0);
        assert_eq!(detail.recordings.total, 1);
        assert!(!detail.recordings.items[0].available);
        assert_eq!(detail.recordings.items[0].artist, artist.name);
        if artist.name == "Personal artist" {
            assert_eq!(artist.id, "stable-private-credit");
            assert_eq!(detail.recordings.items[0].id, "private-track");
        } else {
            assert_eq!(artist.name, "Observed artist");
            assert_eq!(detail.recordings.items[0].id, "prior-track");
        }
    }
    let legacy = hito
        .catalog
        .recordings(
            &actor,
            hito_core::Search {
                library: Some(legacy::LIBRARY.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(legacy.items[0].id, legacy::RECORDING);
    assert!(legacy.items[0].available);
    assert_eq!(
        support::audio_bytes(
            hito.catalog
                .audio_file(&actor, legacy::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&fixture.original).unwrap()
    );
    assert_eq!(
        hito.collection
            .playlist(&actor, legacy::PLAYLIST)
            .await
            .unwrap()
            .entries[0]
            .recording_id,
        legacy::RECORDING
    );
}
