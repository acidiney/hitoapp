#![cfg(target_os = "linux")]

#[path = "support/sqlite_clock.rs"]
mod sqlite_clock;
#[path = "support/sqlite_work.rs"]
mod sqlite_work;

use std::time::Duration;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use hito_community::{Config, Service, router};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad";
const NOW: i64 = 1_900_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Statistics {
    Absent,
    Inherited,
    Fresh,
}

#[tokio::test]
async fn real_admission_work_is_bounded_across_reporter_history_and_fresh_connection_statistics() {
    let clock = sqlite_clock::Clock::new(NOW);
    let _registration = sqlite_work::Registration::new();
    for statistics in [Statistics::Absent, Statistics::Inherited, Statistics::Fresh] {
        for history in [1_000_i64, 100_000] {
            clock.set(NOW);
            let fixture = tempfile::tempdir().unwrap();
            let directory = fixture.path().join("private-rate-work-state");
            let config = || Config {
                state_directory: directory.clone(),
                admin_token: ADMIN.to_owned(),
            };
            let service = Service::open(config()).await.unwrap();
            let app = router(service.clone());
            let (status, owner) = post(&app, "/v1/admin/reporters", ADMIN, json!({})).await;
            assert_eq!(status, 201);
            let (status, peer) = post(&app, "/v1/admin/reporters", ADMIN, json!({})).await;
            assert_eq!(status, 201);
            let owner_id = owner["reporter_id"].as_str().unwrap();
            let owner_token = owner["token"].as_str().unwrap();
            let peer_id = peer["reporter_id"].as_str().unwrap();
            let peer_token = peer["token"].as_str().unwrap();
            service.close().await;
            drop(app);
            drop(service);
            let options = SqliteConnectOptions::new()
                .filename(directory.join("community.sqlite"))
                .foreign_keys(true)
                .disable_statement_logging();
            let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
            insert(
                &mut connection,
                owner_id,
                "50000000",
                "60000000",
                1,
                299,
                NOW,
            )
            .await;
            for (id, submission, receipt) in [
                (owner_id, "10000000", "20000000"),
                (peer_id, "30000000", "40000000"),
            ] {
                insert(&mut connection, id, submission, receipt, 1, 32, NOW - 3600).await;
            }
            if statistics == Statistics::Inherited {
                sqlx::query("ANALYZE")
                    .execute(&mut connection)
                    .await
                    .unwrap();
            }
            for (id, submission, receipt) in [
                (owner_id, "10000000", "20000000"),
                (peer_id, "30000000", "40000000"),
            ] {
                insert(
                    &mut connection,
                    id,
                    submission,
                    receipt,
                    33,
                    history,
                    NOW - 3600,
                )
                .await;
            }
            if statistics == Statistics::Fresh {
                sqlx::query("ANALYZE")
                    .execute(&mut connection)
                    .await
                    .unwrap();
            }
            assert_statistics(&mut connection, statistics, history).await;
            let before = snapshot(&mut connection, "00000000-0000-4000-8000-000000000000").await;
            assert_eq!(before.1.len() as i64, 2 * history + 299);
            connection.close().await.unwrap();

            let service = Service::open(config()).await.unwrap();
            let app = router(service.clone());
            let submission = Uuid::new_v4();
            let measurement = sqlite_work::Measurement::start();
            let (status, receipt) =
                post(&app, "/v1/outcomes", owner_token, claim(submission, "42")).await;
            let (steps, statement) = measurement.finish();
            println!(
                "community_rate_work statistics={statistics:?} history={history} steps={steps} status={status}"
            );
            assert_eq!(
                status, 201,
                "Actual public admission exceeded its fixed work budget"
            );
            assert!(steps <= sqlite_work::BUDGET);
            let measurement = sqlite_work::Measurement::start();
            let (status, rejection) = post(
                &app,
                "/v1/outcomes",
                owner_token,
                claim(Uuid::new_v4(), "44"),
            )
            .await;
            let (full_steps, _) = measurement.finish();
            assert_eq!(status, 429);
            assert_eq!(rejection, json!({"error": "rate_limited"}));
            assert!(full_steps <= sqlite_work::BUDGET);
            assert_eq!(
                post(&app, "/v1/outcomes", owner_token, claim(submission, "42")).await,
                (200, receipt)
            );
            let owner_old = Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap();
            let peer_old = Uuid::parse_str("30000000-0000-4000-8000-000000000001").unwrap();
            assert_eq!(
                post(&app, "/v1/outcomes", owner_token, claim(owner_old, "42"))
                    .await
                    .1["receipt_id"],
                "20000000-0000-4000-8000-000000000001"
            );
            assert_eq!(
                post(&app, "/v1/outcomes", peer_token, claim(peer_old, "42"))
                    .await
                    .1["receipt_id"],
                "40000000-0000-4000-8000-000000000001"
            );
            service.close().await;
            drop(app);
            drop(service);

            let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
            assert_statistics(&mut connection, statistics, history).await;
            assert!(
                snapshot(&mut connection, &submission.to_string()).await == before,
                "Admission changed retained historical rows, rowids or credentials"
            );
            let plan: Vec<(i64, i64, i64, String)> =
                sqlx::query_as(&format!("EXPLAIN QUERY PLAN {statement}"))
                    .bind(owner_id)
                    .bind(300_i64)
                    .fetch_all(&mut connection)
                    .await
                    .unwrap();
            assert!(
                plan.iter().any(
                    |row| row.3.contains("COVERING INDEX outcomes_reporter_created")
                        && row.3.contains("reporter_id=?")
                        && row.3.contains("created_at>?")
                ),
                "The actual traced admission decision must use the reporter/time covering range"
            );
            assert!(clock.reads() > 0);
            connection.close().await.unwrap();
        }
    }
}

async fn insert(
    connection: &mut SqliteConnection,
    reporter: &str,
    submission: &str,
    receipt: &str,
    first: i64,
    last: i64,
    created: i64,
) {
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?) INSERT INTO outcomes(reporter_id,submission_id,receipt_id,provider,recording_id,created_at) SELECT ?,printf('%s-0000-4000-8000-%012x',?,i),printf('%s-0000-4000-8000-%012x',?,i),'jamendo','42',? FROM n")
        .bind(first).bind(last).bind(reporter).bind(submission).bind(receipt).bind(created).execute(connection).await.unwrap();
}

type Reporters = Vec<(i64, String, Vec<u8>, i64)>;
type Outcomes = Vec<(i64, String, String, String, String, String, i64)>;

async fn snapshot(connection: &mut SqliteConnection, excluded: &str) -> (Reporters, Outcomes) {
    let reporters =
        sqlx::query_as("SELECT rowid,id,token_hash,created_at FROM reporters ORDER BY rowid")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    let outcomes = sqlx::query_as("SELECT rowid,reporter_id,submission_id,receipt_id,provider,recording_id,created_at FROM outcomes WHERE submission_id != ? ORDER BY rowid").bind(excluded).fetch_all(connection).await.unwrap();
    (reporters, outcomes)
}

async fn assert_statistics(
    connection: &mut SqliteConnection,
    statistics: Statistics,
    history: i64,
) {
    let tables: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name='sqlite_stat1'")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    if statistics == Statistics::Absent {
        assert_eq!(tables, 0);
        return;
    }
    assert_eq!(tables, 1);
    let stat: String =
        sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='outcomes_ranking'")
            .fetch_one(connection)
            .await
            .unwrap();
    assert_eq!(
        stat.split_whitespace()
            .next()
            .unwrap()
            .parse::<i64>()
            .unwrap(),
        if statistics == Statistics::Inherited {
            363
        } else {
            2 * history + 299
        }
    );
}

fn claim(submission: Uuid, recording: &str) -> Value {
    json!({"submission_id": submission, "source": {"provider": "jamendo", "recording_id": recording}, "outcome": "successful_playback"})
}

async fn post(app: &Router, path: &str, token: &str, body: Value) -> (u16, Value) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body = to_bytes(response.into_body(), 16_384).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    })
    .await
    .expect("Rate work public request exceeded its deadline")
}
