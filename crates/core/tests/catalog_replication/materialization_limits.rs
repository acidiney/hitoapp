use crate::{
    Error, LocalRecordingFact, LocalSourceFact, ServerRelayObservation,
    test_support::{hang_guard::hang_guard, support},
};

struct Fixture {
    _directory: tempfile::TempDir,
    services: crate::Services,
    actor: crate::Actor,
    library: String,
    recording_id: String,
    source_id: String,
}

async fn fixture() -> Fixture {
    let (directory, services, actor, library, _music) = hang_guard(
        "the materialization limits fixture must open and scan",
        Box::pin(async {
            let owned = support::library("issue146-materialization-limits").await;
            support::write_wav(&owned.4.join("owned.wav"), 0);
            support::scan(&owned.1, &owned.2, &owned.3).await;
            owned
        }),
    )
    .await;
    let recording_id: String =
        sqlx::query_scalar("SELECT id FROM recordings WHERE library_id=? ORDER BY id LIMIT 1")
            .bind(&library)
            .fetch_one(&services.catalog_replication.store.pool)
            .await
            .unwrap();
    let source_id: String = sqlx::query_scalar(
        "SELECT id FROM sources WHERE recording_id=? AND provider='local' ORDER BY id LIMIT 1",
    )
    .bind(&recording_id)
    .fetch_one(&services.catalog_replication.store.pool)
    .await
    .unwrap();
    Fixture {
        _directory: directory,
        services,
        actor,
        library,
        recording_id,
        source_id,
    }
}

async fn assert_public_begin_rejects(fixture: &Fixture) {
    assert!(matches!(
        fixture
            .services
            .catalog_replication
            .begin(&fixture.actor, &fixture.library)
            .await,
        Err(Error::Conflict(_))
    ));
    fixture.services.catalog_replication.close().await.unwrap();
    fixture
        .services
        .catalog_replication
        .store
        .pool
        .close()
        .await;
}

#[tokio::test]
async fn materialization_limits_reject_source_count_text_and_encoded_fact_overages() {
    let count = fixture().await;
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<128)
         INSERT INTO sources(id,recording_id,provider,provider_id,ready)
         SELECT 'limit-source-'||i,?,'local','limit-provider-'||i,0 FROM n",
    )
    .bind(&count.recording_id)
    .execute(&count.services.catalog_replication.store.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO source_metadata(source_id,title,artist,duration_ms)
         SELECT id,'bounded source','bounded artist',30000 FROM sources
         WHERE recording_id=? AND id LIKE 'limit-source-%'",
    )
    .bind(&count.recording_id)
    .execute(&count.services.catalog_replication.store.pool)
    .await
    .unwrap();
    let local_sources: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sources WHERE recording_id=? AND provider='local'",
    )
    .bind(&count.recording_id)
    .fetch_one(&count.services.catalog_replication.store.pool)
    .await
    .unwrap();
    assert_eq!(local_sources, 129);
    assert_public_begin_rejects(&count).await;

    let source_text = fixture().await;
    let title = "t".repeat(6000);
    let artist = "a".repeat(5000);
    let source_text_limit = super::super::materialization::MAX_FACT_BYTES / 6;
    assert!(source_text.source_id.len() + title.len() + artist.len() > source_text_limit);
    sqlx::query("UPDATE source_metadata SET title=?,artist=? WHERE source_id=?")
        .bind(&title)
        .bind(&artist)
        .bind(&source_text.source_id)
        .execute(&source_text.services.catalog_replication.store.pool)
        .await
        .unwrap();
    assert_public_begin_rejects(&source_text).await;

    let encoded = fixture().await;
    let escaped = "\u{1}".repeat(5000);
    let preflight_limit = super::super::materialization::MAX_FACT_BYTES / 6;
    let recording_text_bytes =
        encoded.recording_id.len() + encoded.library.len() + escaped.len() * 2;
    let source_text_bytes = encoded.source_id.len() + escaped.len() * 2;
    assert!(recording_text_bytes < preflight_limit);
    assert!(source_text_bytes < preflight_limit);
    sqlx::query("UPDATE recordings SET title=?,artist=? WHERE id=? AND library_id=?")
        .bind(&escaped)
        .bind(&escaped)
        .bind(&encoded.recording_id)
        .bind(&encoded.library)
        .execute(&encoded.services.catalog_replication.store.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE source_metadata SET title=?,artist=? WHERE source_id=?")
        .bind(&escaped)
        .bind(&escaped)
        .bind(&encoded.source_id)
        .execute(&encoded.services.catalog_replication.store.pool)
        .await
        .unwrap();
    let fact = LocalRecordingFact {
        id: encoded.recording_id.clone(),
        library_id: encoded.library.clone(),
        title: escaped.clone(),
        artist: escaped.clone(),
        duration_ms: 50000,
        sources: vec![LocalSourceFact {
            id: encoded.source_id.clone(),
            observation_id: None,
            title: escaped.clone(),
            artist: escaped,
            duration_ms: 50000,
            server_relay: ServerRelayObservation::Unavailable,
            release: None,
        }],
    };
    assert!(
        serde_json::to_vec(&fact).unwrap().len() > super::super::materialization::MAX_FACT_BYTES
    );
    assert_public_begin_rejects(&encoded).await;
}
