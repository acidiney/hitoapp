#[path = "../fixtures/source_preparation_query_plans.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::Fixture;
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteRow};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const REQUESTS: &str = include_str!("../../src/catalog/provider/preparation/requests.rs");
const LEASE: &str = include_str!("../../src/catalog/provider/preparation/lease.rs");

#[derive(Clone, Debug)]
enum Cell {
    Text(String),
    Number(i64),
}

struct Probe {
    name: &'static str,
    sql: &'static str,
    arguments: Vec<Cell>,
    expected: Vec<Vec<Cell>>,
}

fn statement(source: &'static str, scope: &str, call: &str) -> &'static str {
    let scope = source
        .find(scope)
        .expect("the production query scope must exist");
    let call = scope
        + source[scope..]
            .find(call)
            .expect("the production SQL call must exist");
    let start = call + source[call..].find('"').unwrap() + 1;
    let end = start + source[start..].find('"').unwrap();
    let sql = &source[start..end];
    assert!(sql.starts_with("SELECT"));
    assert!(
        !sql.contains('\\'),
        "the diagnostic must decode escaped SQL literals before using them"
    );
    sql
}

#[tokio::test]
async fn retained_history_does_not_expand_preparation_admission_or_worker_query_work() {
    for statistics in ["none", "before-growth", "after-growth"] {
        let fixture = Fixture::start().await;
        let mut connection = fixture.connection().await;
        fixture.active(&mut connection).await;
        fixture.history(&mut connection, 32, 32).await;
        if statistics == "before-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = fixture.connection().await;
        let small = suite(&mut connection, &fixture, 32, 32, true, statistics).await;
        fixture.history(&mut connection, 20_000, 50_000).await;
        if statistics == "after-growth" {
            sqlx::query("ANALYZE")
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let mut connection = fixture.connection().await;
        let large = suite(&mut connection, &fixture, 20_000, 50_000, true, statistics).await;
        for ((name, small), (large_name, large)) in small.iter().zip(&large) {
            assert_eq!(name, large_name);
            assert!(
                *large <= small + 2000,
                "{statistics} {name}: history grew VM work from {small} to {large}"
            );
        }
        Fixture::retire_active(&mut connection).await;
        connection.close().await.unwrap();
        let mut connection = fixture.connection().await;
        suite(&mut connection, &fixture, 20_000, 50_000, false, statistics).await;
        fixture.exceed_pending_limits(&mut connection).await;
        check(
            &mut connection,
            Probe {
                name: "pending-limits",
                sql: statement(REQUESTS, "async fn pending_capacity(", "sqlx::query_as("),
                arguments: vec![
                    Cell::Text(fixture.account.clone()),
                    Cell::Number(16),
                    Cell::Number(128),
                ],
                expected: vec![vec![Cell::Number(16), Cell::Number(128)]],
            },
            statistics,
            20_000,
            true,
        )
        .await;
        connection.close().await.unwrap();
    }
}

async fn suite(
    connection: &mut SqliteConnection,
    fixture: &Fixture,
    history: i64,
    receipts: i64,
    active: bool,
    statistics: &str,
) -> Vec<(&'static str, u64)> {
    let text = |value: &str| Cell::Text(value.into());
    let number = Cell::Number;
    let target = || {
        vec![
            text(&fixture.account),
            text(&fixture.recording),
            text(&fixture.source),
        ]
    };
    let scalar = |value: &str| vec![vec![text(value)]];
    let retained = if active {
        scalar(&fixture.preparation)
    } else {
        vec![]
    };
    let probes = [
        Probe {
            name: "receipt-first",
            sql: statement(REQUESTS, "async fn replay(", "sqlx::query_as("),
            arguments: vec![
                text(&fixture.account),
                text("00000000-0000-4000-8000-000000000001"),
            ],
            expected: vec![vec![text(&"0".repeat(64)), text(&fixture.preparation)]],
        },
        Probe {
            name: "receipt-last",
            sql: statement(REQUESTS, "async fn replay(", "sqlx::query_as("),
            arguments: vec![
                text(&fixture.account),
                text(&format!("00000000-0000-4000-8000-{receipts:012x}")),
            ],
            expected: vec![vec![text(&"0".repeat(64)), text(&fixture.preparation)]],
        },
        Probe {
            name: "latest",
            sql: statement(REQUESTS, "async fn latest(", "sqlx::query_scalar("),
            arguments: target(),
            expected: scalar(&format!("history-{history}")),
        },
        Probe {
            name: "retained",
            sql: statement(REQUESTS, "let retained:", "sqlx::query_scalar("),
            arguments: target(),
            expected: retained,
        },
        Probe {
            name: "unresolved-missing",
            sql: statement(
                REQUESTS,
                "async fn unresolved_origin(",
                "sqlx::query_scalar(",
            ),
            arguments: vec![
                text(&fixture.account),
                text(&fixture.service),
                text("org.hito.preparation-history-fixture"),
                text("owned:recording/first"),
                text("owned:immutable-exact-file/sha256-1"),
            ],
            expected: vec![vec![number(0)]],
        },
        Probe {
            name: "unresolved-existing",
            sql: statement(
                REQUESTS,
                "async fn unresolved_origin(",
                "sqlx::query_scalar(",
            ),
            arguments: vec![
                text("query-account-0"),
                text(&fixture.service),
                text("org.hito.preparation-history-fixture"),
                text("owned:recording/first"),
                text("query-source-0"),
            ],
            expected: vec![vec![number(i64::from(active))]],
        },
        Probe {
            name: "pending",
            sql: statement(REQUESTS, "async fn pending_capacity(", "sqlx::query_as("),
            arguments: vec![text(&fixture.account), number(16), number(128)],
            expected: vec![vec![
                number(i64::from(active)),
                number(if active { 3 } else { 0 }),
            ]],
        },
        Probe {
            name: "eligible",
            sql: statement(LEASE, "let eligible:", "sqlx::query_scalar("),
            arguments: vec![],
            expected: vec![vec![number(i64::from(active))]],
        },
        Probe {
            name: "expired",
            sql: statement(LEASE, "let expired:", "sqlx::query_scalar("),
            arguments: vec![],
            expected: if active {
                scalar("query-expired")
            } else {
                vec![]
            },
        },
        Probe {
            name: "stale",
            sql: statement(LEASE, "let stale:", "sqlx::query_scalar("),
            arguments: vec![],
            expected: if active {
                vec![vec![text("query-expired")], vec![text("query-stale")]]
            } else {
                vec![]
            },
        },
        Probe {
            name: "claim",
            sql: statement(LEASE, "let id: Option<String>", "sqlx::query_scalar("),
            arguments: vec![],
            expected: if active {
                scalar(&fixture.preparation)
            } else {
                vec![]
            },
        },
    ];
    let mut work = Vec::new();
    for probe in probes {
        work.push((
            probe.name,
            check(connection, probe, statistics, history, active).await,
        ));
    }
    work
}

async fn check(
    connection: &mut SqliteConnection,
    probe: Probe,
    statistics: &str,
    history: i64,
    active: bool,
) -> u64 {
    let explain = format!("EXPLAIN QUERY PLAN {}", probe.sql);
    let mut plan = sqlx::query(&explain);
    for argument in &probe.arguments {
        plan = match argument {
            Cell::Text(value) => plan.bind(value),
            Cell::Number(value) => plan.bind(*value),
        };
    }
    let plan: Vec<String> = plan
        .fetch_all(&mut *connection)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect();
    for detail in &plan {
        if probe.name == "latest" {
            assert!(
                !detail.contains("TEMP B-TREE"),
                "latest must use the rowid tie-break without a history sort: {plan:?}"
            );
        }
        let mut words = detail.split_whitespace();
        if words.next() == Some("SCAN")
            && matches!(
                words.next(),
                Some(
                    "source_preparations"
                        | "preparation"
                        | "active"
                        | "source_preparation_receipts"
                )
            )
        {
            assert!(
                [
                    "source_preparation_due",
                    "source_preparation_expired",
                    "source_preparation_leased_account",
                    "source_preparation_pending",
                    "source_preparation_authority_pending",
                ]
                .iter()
                .any(|index| detail.contains(index)),
                "{} must not scan retained history: {plan:?}",
                probe.name
            );
        }
    }
    let steps = Arc::new(AtomicU64::new(0));
    let progress = Arc::clone(&steps);
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            progress.fetch_add(100, Ordering::Relaxed) + 100 < 20_000
        });
    let mut query = sqlx::query(probe.sql);
    for argument in &probe.arguments {
        query = match argument {
            Cell::Text(value) => query.bind(value),
            Cell::Number(value) => query.bind(*value),
        };
    }
    let result = query.fetch_all(&mut *connection).await;
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    let steps = steps.load(Ordering::Relaxed);
    assert!(
        result.is_ok(),
        "{} exceeded 20,000 VM steps or failed with {statistics}, history={history}, active={active}, steps={steps}, plan={plan:?}, error={:?}",
        probe.name,
        result.as_ref().err()
    );
    let rows: Vec<SqliteRow> = result.unwrap();
    assert_eq!(
        rows.len(),
        probe.expected.len(),
        "{} returned the wrong rows",
        probe.name
    );
    for (row, expected) in rows.iter().zip(&probe.expected) {
        for (column, expected) in expected.iter().enumerate() {
            match expected {
                Cell::Text(expected) => assert_eq!(
                    row.get::<String, _>(column),
                    *expected,
                    "{} column {column}",
                    probe.name
                ),
                Cell::Number(expected) => assert_eq!(
                    row.get::<i64, _>(column),
                    *expected,
                    "{} column {column}",
                    probe.name
                ),
            }
        }
    }
    eprintln!(
        "Preparation query {} statistics={statistics} history={history} active={active} VM_steps~{steps}",
        probe.name
    );
    steps
}
