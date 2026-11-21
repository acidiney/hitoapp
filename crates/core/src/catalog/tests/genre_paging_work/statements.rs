use crate::{
    Actor, HostDeviceId,
    catalog::{
        genre_tracks::{ANCHOR_TITLE_SQL, accessible_query, candidates_query},
        genres::GenrePosition,
    },
    test_support::work_meter::WorkMeter,
};
use sqlx::{Execute, QueryBuilder, Sqlite, SqlitePool, Transaction, sqlite::SqliteArguments};
use std::time::Duration;

const STATEMENT_TIMEOUT: Duration = Duration::from_secs(5);
type PlanNode = (i64, i64, i64, String);
type StatementPlan = Result<Vec<PlanNode>, String>;

pub(super) struct StatementExpectations {
    pub(super) anchor_id: String,
    pub(super) anchor_title: String,
    pub(super) first_candidate_ids: Vec<String>,
    pub(super) late_candidate_ids: Vec<String>,
}

pub(super) struct StatementOutcome<T> {
    result: Result<T, String>,
    vm_steps: u64,
}

pub(super) struct StatementDiagnostics {
    accessible: StatementOutcome<Option<String>>,
    first_candidates: StatementOutcome<Vec<(String, String)>>,
    late_anchor: StatementOutcome<Option<String>>,
    late_candidates: StatementOutcome<Vec<(String, String)>>,
    accessible_plan: StatementPlan,
    first_candidate_plan: StatementPlan,
    late_anchor_plan: StatementPlan,
    late_candidate_plan: StatementPlan,
    expected: StatementExpectations,
    transaction: Result<(), String>,
}

fn statement_result<T>(
    result: Result<Result<T, sqlx::Error>, tokio::time::error::Elapsed>,
    vm_steps: u64,
) -> StatementOutcome<T> {
    let result = match result {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("statement timed out".to_owned()),
    };
    StatementOutcome { result, vm_steps }
}

async fn explain_sql(
    transaction: &mut Transaction<'_, Sqlite>,
    sql: &str,
    arguments: SqliteArguments<'_>,
) -> Result<Vec<PlanNode>, String> {
    let statement = format!("EXPLAIN QUERY PLAN {sql}");
    tokio::time::timeout(
        STATEMENT_TIMEOUT,
        sqlx::query_as_with::<_, PlanNode, _>(&statement, arguments).fetch_all(&mut **transaction),
    )
    .await
    .map_err(|_| "query plan timed out".to_owned())?
    .map_err(|error| error.to_string())
}

async fn explain_builder(
    transaction: &mut Transaction<'_, Sqlite>,
    mut query: QueryBuilder<'static, Sqlite>,
) -> Result<Vec<PlanNode>, String> {
    let sql = query.sql().to_owned();
    let arguments = query
        .build()
        .take_arguments()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "query builder has no bind arguments".to_owned())?;
    explain_sql(transaction, &sql, arguments).await
}

pub(super) async fn collect(
    pool: &SqlitePool,
    host: &HostDeviceId,
    actor: &Actor,
    expected: StatementExpectations,
    meter: &WorkMeter,
) -> Result<StatementDiagnostics, String> {
    let mut transaction = tokio::time::timeout(STATEMENT_TIMEOUT, pool.begin())
        .await
        .map_err(|_| "diagnostic transaction start timed out".to_owned())?
        .map_err(|error| error.to_string())?;

    let accessible_plan =
        explain_builder(&mut transaction, accessible_query(host, actor, "fado")).await;
    let first_candidate_plan = explain_builder(
        &mut transaction,
        candidates_query(host, actor, "fado", super::PAGE_SIZE, None),
    )
    .await;

    let (accessible, accessible_steps) = meter
        .measure(tokio::time::timeout(STATEMENT_TIMEOUT, async {
            accessible_query(host, actor, "fado")
                .build_query_scalar::<String>()
                .fetch_optional(&mut *transaction)
                .await
        }))
        .await;
    let accessible = statement_result(accessible, accessible_steps);

    let (first_candidates, first_candidate_steps) = meter
        .measure(tokio::time::timeout(STATEMENT_TIMEOUT, async {
            candidates_query(host, actor, "fado", super::PAGE_SIZE, None)
                .build_query_as::<(String, String)>()
                .fetch_all(&mut *transaction)
                .await
        }))
        .await;
    let first_candidates = statement_result(first_candidates, first_candidate_steps);

    let mut anchor_query = sqlx::query(ANCHOR_TITLE_SQL)
        .bind(actor.id().to_owned())
        .bind(expected.anchor_id.to_owned());
    let anchor_arguments = anchor_query.take_arguments();
    let late_anchor_plan = match anchor_arguments {
        Ok(Some(arguments)) => explain_sql(&mut transaction, ANCHOR_TITLE_SQL, arguments).await,
        Ok(None) => Err("anchor query has no bind arguments".to_owned()),
        Err(error) => Err(error.to_string()),
    };

    let (late_anchor, late_anchor_steps) = meter
        .measure(tokio::time::timeout(STATEMENT_TIMEOUT, async {
            sqlx::query_scalar::<_, String>(ANCHOR_TITLE_SQL)
                .bind(actor.id().to_owned())
                .bind(expected.anchor_id.to_owned())
                .fetch_optional(&mut *transaction)
                .await
        }))
        .await;
    let late_anchor = statement_result(late_anchor, late_anchor_steps);
    let late_title = late_anchor
        .result
        .as_ref()
        .ok()
        .and_then(Option::as_deref)
        .unwrap_or(expected.anchor_title.as_str())
        .to_owned();
    let late_position = GenrePosition {
        title: &late_title,
        id: expected.anchor_id.as_str(),
    };
    let late_candidate_plan = explain_builder(
        &mut transaction,
        candidates_query(host, actor, "fado", super::PAGE_SIZE, Some(late_position)),
    )
    .await;
    let (late_candidates, late_candidate_steps) = meter
        .measure(tokio::time::timeout(STATEMENT_TIMEOUT, async {
            candidates_query(host, actor, "fado", super::PAGE_SIZE, Some(late_position))
                .build_query_as::<(String, String)>()
                .fetch_all(&mut *transaction)
                .await
        }))
        .await;
    let late_candidates = statement_result(late_candidates, late_candidate_steps);

    let transaction = tokio::time::timeout(STATEMENT_TIMEOUT, transaction.commit())
        .await
        .map_err(|_| "diagnostic transaction commit timed out".to_owned())?
        .map_err(|error| error.to_string());

    Ok(StatementDiagnostics {
        accessible,
        first_candidates,
        late_anchor,
        late_candidates,
        accessible_plan,
        first_candidate_plan,
        late_anchor_plan,
        late_candidate_plan,
        expected,
        transaction,
    })
}

pub(super) fn assert_and_log(size: usize, statements: &StatementDiagnostics) {
    assert!(statements.transaction.is_ok());
    assert!(statements.accessible.result.is_ok());
    assert!(statements.accessible.result.as_ref().unwrap().is_some());
    assert!(statements.first_candidates.result.is_ok());
    assert!(statements.first_candidates.vm_steps > 0);
    assert_eq!(
        statements
            .first_candidates
            .result
            .as_ref()
            .unwrap()
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>(),
        statements.expected.first_candidate_ids
    );
    assert!(statements.late_anchor.result.is_ok());
    assert_eq!(
        statements.late_anchor.result.as_ref().unwrap().as_deref(),
        Some(statements.expected.anchor_title.as_str())
    );
    assert!(statements.late_candidates.result.is_ok());
    assert!(statements.late_candidates.vm_steps > 0);
    assert_eq!(
        statements
            .late_candidates
            .result
            .as_ref()
            .unwrap()
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>(),
        statements.expected.late_candidate_ids
    );
    for plan in [
        &statements.accessible_plan,
        &statements.first_candidate_plan,
        &statements.late_anchor_plan,
        &statements.late_candidate_plan,
    ] {
        assert!(plan.is_ok());
        assert!(!plan.as_ref().unwrap().is_empty());
    }

    eprintln!(
        "genre_page_statement case=file-tag size={size} statement=accessible vm_steps={} row_count={}",
        statements.accessible.vm_steps,
        if statements.accessible.result.as_ref().unwrap().is_some() {
            1
        } else {
            0
        }
    );
    eprintln!(
        "genre_page_statement case=file-tag size={size} statement=candidates-first vm_steps={} row_count={}",
        statements.first_candidates.vm_steps,
        statements.first_candidates.result.as_ref().unwrap().len()
    );
    eprintln!(
        "genre_page_statement case=file-tag size={size} statement=anchor-late vm_steps={} row_count={}",
        statements.late_anchor.vm_steps,
        if statements.late_anchor.result.as_ref().unwrap().is_some() {
            1
        } else {
            0
        }
    );
    eprintln!(
        "genre_page_statement case=file-tag size={size} statement=candidates-late vm_steps={} row_count={}",
        statements.late_candidates.vm_steps,
        statements.late_candidates.result.as_ref().unwrap().len()
    );
    for (name, plan) in [
        ("accessible", &statements.accessible_plan),
        ("candidates-first", &statements.first_candidate_plan),
        ("anchor-late", &statements.late_anchor_plan),
        ("candidates-late", &statements.late_candidate_plan),
    ] {
        eprintln!(
            "genre_page_eqp case=file-tag size={size} statement={name} nodes={:?}",
            plan.as_ref().unwrap()
        );
    }
}
