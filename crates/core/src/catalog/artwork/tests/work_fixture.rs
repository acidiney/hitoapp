pub(super) use crate::test_support::collection_artwork as provider;

use crate::{Credentials, ImportProviderPlaylist, Services};
use sqlx::SqliteConnection;
use std::path::Path;
use uuid::Uuid;

pub(super) const HISTORY_ROWS: i64 = 336;

pub(super) async fn copy_database(hito: &Services, destination: &Path) {
    let image = hito
        .catalog
        .store
        .pool
        .acquire()
        .await
        .unwrap()
        .serialize(None)
        .await
        .unwrap();
    std::fs::write(destination, &*image).unwrap();
}

pub(super) struct Expected {
    pub owner_token: String,
    pub other_token: String,
    pub recording: String,
    pub release: String,
    pub receipt: String,
}

pub(super) async fn seed(hito: &Services, server: &provider::ArtworkServer) -> Expected {
    let session = hito
        .identity
        .setup(credentials("artwork-work-owner"))
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    hito.identity
        .create_user(&actor, credentials("artwork-work-other"))
        .await
        .unwrap();
    let other = hito
        .identity
        .login(credentials("artwork-work-other"))
        .await
        .unwrap();
    let installed = server.connect(hito, &actor).await;
    hito.collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork-work".into(),
            },
        )
        .await
        .unwrap();
    let recording = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let receipt = hito
        .catalog
        .enqueue_recording_artwork(&actor, &recording.id)
        .await
        .unwrap();
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    Expected {
        owner_token: session.token,
        other_token: other.token,
        recording: recording.id,
        release: recording.release_id.unwrap(),
        receipt: receipt.id,
    }
}

pub(super) async fn grow(hito: &Services, recording: &str, start: i64, end: i64) {
    let mut tx = hito.catalog.store.pool.begin().await.unwrap();
    context(&mut tx, recording).await;
    sqlx::query("CREATE TEMP TABLE artwork_work_indices(i INTEGER PRIMARY KEY)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("WITH RECURSIVE fixture(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM fixture WHERE i+1 < ?) INSERT INTO artwork_work_indices SELECT i FROM fixture")
        .bind(start).bind(end).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!("work_fixture.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql("DROP TABLE artwork_work_indices; DROP TABLE artwork_work_context;")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn history(hito: &Services, recording: &str) {
    let mut tx = hito.catalog.store.pool.begin().await.unwrap();
    context(&mut tx, recording).await;
    let inserted = sqlx::query("WITH RECURSIVE fixture(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM fixture WHERE i<335)
        INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,status,attempts,reservation_bytes,created_at,updated_at)
        SELECT printf('af500000-0000-4000-8000-%012x',i),j.user_id,j.recording_id,j.session_hash,j.observation_id,j.binding_id,j.binding_revision,j.installation_id,j.installation_revision,j.service_id,j.service_revision,
            'failed',3,0,1,CASE WHEN i<16 THEN unixepoch()-700000 ELSE unixepoch()-i END
        FROM fixture CROSS JOIN artwork_work_context template JOIN artwork_fetches j ON j.id=template.fetch_id")
        .execute(&mut *tx).await.unwrap().rows_affected();
    assert_eq!(
        inserted, HISTORY_ROWS as u64,
        "history seed must insert every owned receipt"
    );
    sqlx::query("DROP TABLE artwork_work_context")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn assert_history(hito: &Services, phase: &str) {
    let (history, unretained): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(retained = 0),0) FROM artwork_fetches WHERE id >= 'af500000-' AND id < 'af500001-'",
    )
    .fetch_one(&hito.catalog.store.pool)
    .await
    .unwrap();
    eprintln!("Artwork fixture history phase={phase} receipts={history} unretained={unretained}");
    assert_eq!(
        (history, unretained),
        (HISTORY_ROWS, HISTORY_ROWS),
        "the {phase} fixture must retain all seeded history before maintenance"
    );
}

async fn context(connection: &mut SqliteConnection, recording: &str) {
    sqlx::query("CREATE TEMP TABLE artwork_work_context AS SELECT a.fetch_id FROM recording_artwork a WHERE a.recording_id=?")
        .bind(recording).execute(connection).await.unwrap();
}

pub(super) async fn assert_retained(hito: &Services, expected: i64) {
    let (recordings, receipts, links, hashes): (i64,i64,i64,i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM recordings),(SELECT count(*) FROM artwork_fetches j JOIN recording_artwork a ON a.fetch_id=j.id WHERE j.status='ready' AND j.entity_kind='recording'),
            (SELECT count(*) FROM recording_artwork),(SELECT count(DISTINCT content_hash) FROM recording_artwork)")
        .fetch_one(&hito.catalog.store.pool).await.unwrap();
    assert_eq!(
        (recordings, receipts, links, hashes),
        (expected, expected, expected, 1)
    );
    let bytes: Vec<Vec<u8>> = sqlx::query_scalar("SELECT bytes FROM artwork_assets WHERE content_hash IN (SELECT content_hash FROM recording_artwork)")
        .fetch_all(&hito.catalog.store.pool).await.unwrap();
    assert_eq!(bytes.len(), 1);
    assert_eq!(bytes[0].as_slice(), provider::RECORDING_COVER);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&hito.catalog.store.pool)
            .await
            .unwrap()
            .is_empty()
    );
}

pub(super) async fn assert_statistics(hito: &Services, expected: Option<(i64, i64)>) {
    for (index, expected) in [
        ("recordings_release", expected.map(|counts| counts.0)),
        ("artwork_account", expected.map(|counts| counts.1)),
    ] {
        let stat: Option<String> = sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx=?")
            .bind(index)
            .fetch_optional(&hito.catalog.store.pool)
            .await
            .unwrap();
        let count = stat
            .as_deref()
            .and_then(|value| value.split_whitespace().next())
            .map(|value| value.parse::<i64>().unwrap());
        assert_eq!(
            count, expected,
            "the reopened probe must use authentic {index} statistics"
        );
    }
}

fn credentials(name: &str) -> Credentials {
    Credentials {
        username: name.into(),
        password: "owned artwork work fixture password".into(),
    }
}
