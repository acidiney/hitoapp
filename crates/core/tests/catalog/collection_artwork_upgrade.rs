#[path = "../fixtures/legacy_library.rs"]
#[allow(dead_code)]
mod legacy;
use hito_core::test_support::collection_artwork as provider;
#[path = "../fixtures/collection_artwork_schema.rs"]
mod schema;
#[path = "../fixtures/source_resolution_evidence_snapshot.rs"]
mod snapshot;
use hito_core::test_support::support;
#[path = "../fixtures/collection_artwork_upgrade.rs"]
mod upgrade;

use hito_core::{ArtworkFetchStatus, Error};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use sqlx::Connection;

#[tokio::test]
async fn historical_artwork_upgrade_preserves_ready_references_pending_authority_and_atomic_rollback()
 {
    let fixture = legacy::Legacy::seed().await;
    let expected_audio = std::fs::read(&fixture.original).unwrap();
    let provider = provider::ArtworkServer::start().await;
    let database = fixture.directory.path().join("hito.db");
    let invalid_database = fixture.directory.path().join("invalid.db");
    let mut connection = upgrade::connect(&database).await;
    upgrade::seed(&mut connection, &provider.url, provider::RECORDING_COVER).await;
    let before = snapshot::capture(&mut connection, 50).await;
    let before_schema = upgrade::schema(&mut connection).await;
    let job_keys = upgrade::foreign_keys(&mut connection, "artwork_fetches").await;
    let retained_keys = upgrade::foreign_keys(&mut connection, "recording_artwork").await;
    let authority: Vec<String> =
        sqlx::query_scalar("SELECT id FROM current_artwork_fetch_authority ORDER BY id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(authority, [upgrade::READY_JOB, upgrade::PENDING_JOB]);
    connection.close().await.unwrap();
    std::fs::copy(&database, &invalid_database).unwrap();

    let hito = match hito_core::open(hito_core::CoreConfig::new(
        database.clone(),
        fixture.music.clone(),
        hito_core::HostDeviceId::new("server").unwrap(),
    ))
    .await
    {
        Ok(hito) => hito,
        Err(Error::Migration(error)) => upgrade::migration_failure(error),
        Err(error) => panic!("Owned artwork fixture startup failed: {}", error.code()),
    };
    let owner = hito
        .identity
        .authenticate(upgrade::OWNER_TOKEN)
        .await
        .unwrap();
    let other = hito
        .identity
        .authenticate(upgrade::OTHER_TOKEN)
        .await
        .unwrap();
    let mut connection = upgrade::connect(&database).await;
    snapshot::assert_preserved(&mut connection, &before).await;
    upgrade::assert_schema_preserved(
        &mut connection,
        &before_schema,
        &[
            schema::erasure_schema::REPLACED,
            schema::account_schema::REPLACED,
        ]
        .concat(),
    )
    .await;
    upgrade::assert_integrity(&mut connection).await;
    let migrated_job_keys = upgrade::foreign_keys(&mut connection, "artwork_fetches").await;
    assert!(
        job_keys.iter().all(|key| migrated_job_keys.contains(key)),
        "every legacy artwork foreign key must survive the generalized job table"
    );
    assert!(upgrade::foreign_keys(&mut connection, "recording_artwork").await == retained_keys);
    schema::assert_current(&mut connection).await;
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM current_artwork_fetch_authority WHERE id=?"
        )
        .bind(upgrade::PENDING_JOB)
        .fetch_one(&mut connection)
        .await
        .unwrap(),
        upgrade::PENDING_JOB
    );
    let ready = hito
        .catalog
        .artwork_fetch(&owner, upgrade::READY_JOB)
        .await
        .unwrap();
    assert!(matches!(ready.status, ArtworkFetchStatus::Ready));
    assert_eq!(ready.attempts, 1);
    let queued = hito
        .catalog
        .artwork_fetch(&other, upgrade::PENDING_JOB)
        .await
        .unwrap();
    assert!(matches!(queued.status, ArtworkFetchStatus::Queued));
    assert_eq!(queued.attempts, 0);
    assert_eq!(queued.recording_id, upgrade::PENDING_RECORDING);
    assert_eq!(
        hito.catalog
            .artwork(&owner, upgrade::READY_RECORDING)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        provider::RECORDING_COVER
    );
    for (actor, recording, job) in [
        (&other, upgrade::READY_RECORDING, upgrade::READY_JOB),
        (&owner, upgrade::PENDING_RECORDING, upgrade::PENDING_JOB),
    ] {
        assert!(matches!(
            hito.catalog.artwork(actor, recording).await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.catalog.artwork_fetch(actor, job).await,
            Err(Error::NotFound)
        ));
    }
    let metadata = serde_json::to_value(
        hito.catalog
            .recording_metadata(&owner, upgrade::READY_RECORDING, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["title"]["personal"], "My ready title");
    assert_eq!(metadata["artist"]["personal"], "My ready artist");
    assert_eq!(
        hito.collection
            .saved_position(
                &owner,
                uuid::Uuid::parse_str(upgrade::READY_RECORDING).unwrap()
            )
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        4200
    );
    assert_eq!(
        support::audio_bytes(
            hito.playback
                .audio_file(&owner, legacy::SOURCE)
                .await
                .unwrap()
        ),
        expected_audio
    );
    assert_eq!(std::fs::read(&fixture.original).unwrap(), expected_audio);
    assert!(provider.requests().await.is_empty());
    snapshot::assert_preserved(&mut connection, &before).await;
    assert_guards(&mut connection).await;
    snapshot::assert_preserved(&mut connection, &before).await;
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    let completed = hito
        .catalog
        .artwork_fetch(&other, upgrade::PENDING_JOB)
        .await
        .unwrap();
    assert_eq!(completed.id, queued.id);
    assert!(matches!(completed.status, ArtworkFetchStatus::Ready));
    assert_eq!(completed.attempts, 1);
    assert_eq!(
        hito.catalog
            .artwork(&other, upgrade::PENDING_RECORDING)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        provider::RECORDING_COVER
    );
    assert_eq!(
        provider.requests().await,
        [ArtworkRequest {
            entity: ArtworkEntity::Recording,
            provider_id: provider::RECORDING_ID.into()
        }]
    );
    upgrade::assert_integrity(&mut connection).await;
    connection.close().await.unwrap();

    let mut invalid = upgrade::connect(&invalid_database).await;
    let mut transaction = invalid.begin().await.unwrap();
    sqlx::query("PRAGMA defer_foreign_keys=ON")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,status,reservation_bytes)
        SELECT 'owned-invalid-history',user_id,'owned-missing-recording',session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,'queued',2097152 FROM artwork_fetches WHERE id=?")
        .bind(upgrade::PENDING_JOB).execute(&mut *transaction).await.unwrap();
    upgrade::through(51)
        .run(&mut *transaction)
        .await
        .expect("deferred historical foreign keys must remain pending until the outer commit");
    assert!(
        transaction.commit().await.is_err(),
        "invalid historical references must reject the migration commit"
    );
    invalid.close().await.unwrap();
    let mut reopened = upgrade::connect(&invalid_database).await;
    snapshot::assert_preserved(&mut reopened, &before).await;
    assert!(
        upgrade::schema(&mut reopened).await == before_schema,
        "a rejected migration must leave no partial schema or migration receipt"
    );
    upgrade::assert_integrity(&mut reopened).await;
    reopened.close().await.unwrap();
}

async fn assert_guards(connection: &mut sqlx::SqliteConnection) {
    let mut transaction = connection.begin().await.unwrap();
    assert!(
        sqlx::query("UPDATE artwork_fetches SET binding_revision=binding_revision+1 WHERE id=?")
            .bind(upgrade::PENDING_JOB)
            .execute(&mut *transaction)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE artwork_fetches SET reservation_bytes=0 WHERE id=?")
            .bind(upgrade::PENDING_JOB)
            .execute(&mut *transaction)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE artwork_fetches SET entity_kind='playlist' WHERE id=?")
            .bind(upgrade::PENDING_JOB)
            .execute(&mut *transaction)
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM sessions WHERE user_id='artwork-other'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM artwork_fetches WHERE id=?")
            .bind(upgrade::PENDING_JOB)
            .fetch_one(&mut *transaction)
            .await
            .unwrap(),
        "cancelled"
    );
    transaction.rollback().await.unwrap();
}
