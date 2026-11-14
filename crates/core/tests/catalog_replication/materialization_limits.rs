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
            support::write_wav(&owned.4.join("earlier.wav"), 0);
            support::write_wav(&owned.4.join("later.wav"), 1);
            support::scan(&owned.1, &owned.2, &owned.3).await;
            sqlx::query(
                "UPDATE recordings SET title=CASE WHEN id=(SELECT id FROM recordings WHERE library_id=? ORDER BY id LIMIT 1) THEN 'Earlier' ELSE 'Later' END WHERE library_id=?",
            )
            .bind(&owned.3)
            .bind(&owned.3)
            .execute(&owned.1.catalog_replication.store.pool)
            .await
            .unwrap();
            owned
        }),
    )
    .await;
    let (recording_id, source_id) = hang_guard(
        "the materialization limits fixture identity must be selected",
        async {
            let recording_id: String = sqlx::query_scalar(
                "SELECT id FROM recordings WHERE library_id=? ORDER BY title DESC,id DESC LIMIT 1",
            )
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
            (recording_id, source_id)
        },
    )
    .await;
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
    {
        let registry = fixture
            .services
            .catalog_replication
            .registry
            .lock()
            .unwrap();
        assert!(registry.snapshots.is_empty());
        assert_eq!(registry.bytes, 0);
        assert!(registry.constructions.is_empty());
    }
    fixture.services.catalog_replication.close().await.unwrap();
    fixture
        .services
        .catalog_replication
        .store
        .pool
        .close()
        .await;
}

async fn assert_later_ordinal_stats(fixture: &Fixture, over_count: bool, over_bytes: bool) {
    let ordered: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT r.id,count(s.id),COALESCE(sum(length(CAST(s.id AS BLOB))+COALESCE(length(CAST(m.observation_id AS BLOB)),0)+length(CAST(m.title AS BLOB))+length(CAST(m.artist AS BLOB))+COALESCE(length(CAST(a.id AS BLOB)),0)+COALESCE(length(CAST(a.title AS BLOB)),0)+COALESCE(length(CAST(a.artist AS BLOB)),0)),0)
         FROM recordings r JOIN sources s ON s.recording_id=r.id AND s.provider='local'
         JOIN source_metadata m ON m.source_id=s.id
         LEFT JOIN releases a ON a.id=m.release_id
         WHERE r.library_id=? GROUP BY r.id ORDER BY r.title,r.id",
    )
    .bind(&fixture.library)
    .fetch_all(&fixture.services.catalog_replication.store.pool)
    .await
    .unwrap();
    assert_eq!(ordered.len(), 2);
    assert_ne!(ordered[0].0, fixture.recording_id);
    assert_eq!(ordered[1].0, fixture.recording_id);
    let threshold = (super::super::materialization::MAX_FACT_BYTES / 6) as i64;
    assert!(ordered[0].1 <= super::super::materialization::MAX_SOURCES);
    assert!(ordered[0].2 <= threshold);
    assert_eq!(
        ordered[1].1 > super::super::materialization::MAX_SOURCES,
        over_count
    );
    assert_eq!(ordered[1].2 > threshold, over_bytes);
}

#[tokio::test]
async fn materialization_limits_reject_source_count_text_and_encoded_fact_overages() {
    let count = fixture().await;
    hang_guard(
        "the materialization source-count guard fixture must be prepared",
        async {
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
            assert_later_ordinal_stats(&count, true, false).await;
        },
    )
    .await;
    assert_public_begin_rejects(&count).await;

    let source_text = fixture().await;
    let title = "t".repeat(6000);
    let artist = "a".repeat(5000);
    let source_text_limit = super::super::materialization::MAX_FACT_BYTES / 6;
    hang_guard(
        "the materialization source-text guard fixture must be prepared",
        async {
            assert!(source_text.source_id.len() + title.len() + artist.len() > source_text_limit);
            sqlx::query("UPDATE source_metadata SET title=?,artist=? WHERE source_id=?")
                .bind(&title)
                .bind(&artist)
                .bind(&source_text.source_id)
                .execute(&source_text.services.catalog_replication.store.pool)
                .await
                .unwrap();
            assert_later_ordinal_stats(&source_text, false, true).await;
        },
    )
    .await;
    assert_public_begin_rejects(&source_text).await;

    let observation = fixture().await;
    let observation_id = "o".repeat(source_text_limit + 100);
    hang_guard(
        "the materialization observation guard fixture must be prepared",
        async {
            let inserted = sqlx::query(
                "INSERT INTO metadata_observations(id,source_id,recording_id,release_id,provider,origin,observed_at,file_checksum,inspection_version,title,artist,album,album_artist,duration_ms,disc_number,track_number,tags)
                 SELECT ?,source_id,recording_id,release_id,provider,origin,observed_at,NULL,999,title,artist,album,album_artist,duration_ms,disc_number,track_number,tags
                 FROM metadata_observations WHERE source_id=? ORDER BY sequence DESC LIMIT 1",
            )
            .bind(&observation_id)
            .bind(&observation.source_id)
            .execute(&observation.services.catalog_replication.store.pool)
            .await
            .unwrap();
            assert_eq!(inserted.rows_affected(), 1);
            sqlx::query("UPDATE source_metadata SET observation_id=? WHERE source_id=?")
                .bind(&observation_id)
                .bind(&observation.source_id)
                .execute(&observation.services.catalog_replication.store.pool)
                .await
                .unwrap();
            assert_later_ordinal_stats(&observation, false, true).await;
        },
    )
    .await;
    assert_public_begin_rejects(&observation).await;

    let release = fixture().await;
    let release_id = "issue146-owned-late-release";
    hang_guard(
        "the materialization release guard fixture must be prepared",
        async {
            sqlx::query(
                "INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES(?,?, 'owned release','owned artist',?)",
            )
            .bind(release_id)
            .bind(&release.library)
            .bind(release_id)
            .execute(&release.services.catalog_replication.store.pool)
            .await
            .unwrap();
            sqlx::query("UPDATE source_metadata SET release_id=? WHERE source_id=?")
                .bind(release_id)
                .bind(&release.source_id)
                .execute(&release.services.catalog_replication.store.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE releases SET title=? WHERE id=?")
                .bind("r".repeat(source_text_limit + 100))
                .bind(release_id)
                .execute(&release.services.catalog_replication.store.pool)
                .await
                .unwrap();
            assert_later_ordinal_stats(&release, false, true).await;
        },
    )
    .await;
    assert_public_begin_rejects(&release).await;

    let aggregate = fixture().await;
    hang_guard(
        "the materialization aggregate guard fixture must be prepared",
        async {
            sqlx::query(
                "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<2)
                 INSERT INTO sources(id,recording_id,provider,provider_id,ready)
                 SELECT 'aggregate-source-'||i,?,'local','aggregate-provider-'||i,0 FROM n",
            )
            .bind(&aggregate.recording_id)
            .execute(&aggregate.services.catalog_replication.store.pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO source_metadata(source_id,title,artist,duration_ms)
                 SELECT id,?, ?,30000 FROM sources WHERE recording_id=? AND id LIKE 'aggregate-source-%'",
            )
            .bind("t".repeat(3000))
            .bind("a".repeat(3000))
            .bind(&aggregate.recording_id)
            .execute(&aggregate.services.catalog_replication.store.pool)
            .await
            .unwrap();
            assert_later_ordinal_stats(&aggregate, false, true).await;
        },
    )
    .await;
    assert_public_begin_rejects(&aggregate).await;

    let encoded = fixture().await;
    let escaped = "\u{1}".repeat(5000);
    let preflight_limit = super::super::materialization::MAX_FACT_BYTES / 6;
    hang_guard(
        "the materialization encoded-fact guard fixture must be prepared",
        async {
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
                    artist: escaped.clone(),
                    duration_ms: 50000,
                    server_relay: ServerRelayObservation::Unavailable,
                    release: None,
                }],
            };
            assert!(
                serde_json::to_vec(&fact).unwrap().len()
                    > super::super::materialization::MAX_FACT_BYTES
            );
        },
    )
    .await;
    assert_public_begin_rejects(&encoded).await;
}

#[tokio::test]
async fn materialization_limits_accept_the_inclusive_source_count_boundary() {
    let fixture = fixture().await;
    hang_guard(
        "the materialization inclusive source-count fixture must be prepared",
        async {
            sqlx::query(
                "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<127)
                 INSERT INTO sources(id,recording_id,provider,provider_id,ready)
                 SELECT 'boundary-source-'||i,?,'local','boundary-provider-'||i,0 FROM n",
            )
            .bind(&fixture.recording_id)
            .execute(&fixture.services.catalog_replication.store.pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO source_metadata(source_id,title,artist,duration_ms)
                 SELECT id,'bounded source','bounded artist',30000 FROM sources
                 WHERE recording_id=? AND id LIKE 'boundary-source-%'",
            )
            .bind(&fixture.recording_id)
            .execute(&fixture.services.catalog_replication.store.pool)
            .await
            .unwrap();
        },
    )
    .await;

    let snapshot = fixture
        .services
        .catalog_replication
        .begin(&fixture.actor, &fixture.library)
        .await
        .unwrap();
    assert_eq!(snapshot.recording_count, 2);
    let page = fixture
        .services
        .catalog_replication
        .page(&fixture.actor, snapshot.id, 0)
        .await
        .unwrap();
    let recording = page
        .recordings
        .iter()
        .find(|recording| recording.id == fixture.recording_id)
        .unwrap();
    assert_eq!(recording.sources.len(), 128);
    fixture.services.catalog_replication.close().await.unwrap();
    fixture
        .services
        .catalog_replication
        .store
        .pool
        .close()
        .await;
}
