#[path = "../fixtures/legacy_library.rs"]
mod legacy;
use hito_core::test_support::support;

use hito_core::{Action, Search};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use sha2::{Digest, Sha256};
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::borrow::Cow;
use uuid::Uuid;

#[tokio::test]
async fn upgrading_title_state_preserves_pins_resets_and_operation_ids() {
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
        migrations: Cow::Owned(all.iter().filter(|m| m.version <= 14).cloned().collect()),
        ..Migrator::DEFAULT
    }
    .run(&pool)
    .await
    .unwrap();
    let operation = Uuid::new_v4();
    let previous_reset = Uuid::new_v4();
    sqlx::query("INSERT INTO personal_recording_titles(user_id,recording_id,override_title,title,artist,operation_id) VALUES ('legacy-admin',?,'Pinned title','Pinned title','Legacy artist',?)")
        .bind(legacy::RECORDING).bind(operation.to_string()).execute(&pool).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO users(id,username,password_hash,admin) VALUES ('bob','bob','unused',0);
        INSERT INTO library_members VALUES ('10000000-0000-4000-8000-000000000003','bob');",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sessions VALUES (?, 'bob', unixepoch() + 86400)")
        .bind(format!("{:x}", Sha256::digest(b"upgrade-bob-token")))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO personal_recording_titles(user_id,recording_id,title,artist,operation_id) VALUES ('bob',?,'Legacy track','Legacy artist',?)")
        .bind(legacy::RECORDING).bind(previous_reset.to_string()).execute(&pool).await.unwrap();
    pool.close().await;
    let (hito, actor) = fixture.open().await;
    let bob = hito
        .identity
        .authenticate("upgrade-bob-token")
        .await
        .unwrap();
    let bob_detail = hito
        .catalog
        .recording_metadata(&bob, legacy::RECORDING, Default::default())
        .await
        .unwrap();
    assert_eq!(bob_detail.title.value, "Legacy track");
    assert!(bob_detail.title.personal.is_none());
    assert_eq!(
        bob_detail.title.operation_id.as_deref(),
        Some(previous_reset.to_string().as_str())
    );
    let detail = hito
        .catalog
        .recording_metadata(&actor, legacy::RECORDING, Default::default())
        .await
        .unwrap();
    assert_eq!(detail.title.value, "Pinned title");
    assert_eq!(
        detail.title.operation_id.as_deref(),
        Some(operation.to_string().as_str())
    );
    assert_eq!(detail.artist.value, "Legacy artist");
    assert!(detail.artist.operation_id.is_none());
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Retagged title".into());
    tag.set_artist("Retagged credit".into());
    tag.save_to_path(&fixture.original, Default::default())
        .unwrap();
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let page = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("Pinned Retagged".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, legacy::RECORDING);
    assert_eq!(page.items[0].artist, "Retagged credit");
    let bob_detail = hito
        .catalog
        .recording_metadata(&bob, legacy::RECORDING, Default::default())
        .await
        .unwrap();
    assert_eq!(bob_detail.title.value, "Retagged title");
    assert!(bob_detail.title.personal.is_none());
    assert_eq!(
        hito.catalog
            .recordings(
                &bob,
                Search {
                    q: Some("Pinned".into()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingTitleInherit {
            recording_id: Uuid::parse_str(legacy::RECORDING).unwrap(),
        }],
    )
    .await;
    let reset = hito
        .catalog
        .recording_metadata(&actor, legacy::RECORDING, Default::default())
        .await
        .unwrap();
    assert!(reset.title.personal.is_none());
    let reset_operation = reset.title.operation_id.unwrap();
    drop(hito);
    let (reopened, actor) = fixture.open().await;
    let reset = reopened
        .catalog
        .recording_metadata(&actor, legacy::RECORDING, Default::default())
        .await
        .unwrap();
    assert_eq!(reset.title.value, "Retagged title");
    assert_eq!(
        reset.title.operation_id.as_deref(),
        Some(reset_operation.as_str())
    );
    assert!(reset.title.personal.is_none());
}
