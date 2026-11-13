use crate::resolution_deleted_receipt_fixture::Fixture;
use futures_util::FutureExt;
use hito_core::Error;
use hito_core::test_support::hang_guard::{HANG_CEILING, hang_guard};
use std::panic::{AssertUnwindSafe, resume_unwind};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_account_losing_its_accepted_resolution_receipt_returns_not_found() {
    let mut fixture = Fixture::new();
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            hang_guard("owned deleted receipt setup", Box::pin(fixture.prepare())).await;
            fixture.exercise().await
        })
        .catch_unwind(),
    )
    .await;
    fixture.retire().await;
    let outcome = match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("the owned deleted receipt exercise must settle within its existing bound")
        }
    };
    let observed = match &outcome {
        Err(Error::NotFound) => "NotFound",
        Err(Error::Database(sqlx::Error::RowNotFound)) => "Database(RowNotFound)",
        Err(_) => "another domain error",
        Ok(_) => "a successful resolution",
    };
    assert!(
        matches!(outcome, Err(Error::NotFound)),
        "a deleted accepted receipt must retain the prior NotFound result after owned cleanup; observed {observed}"
    );
}
