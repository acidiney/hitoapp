mod assertions;
mod fixture;
mod historical;

use fixture::RepairFixture;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

#[tokio::test]
async fn startup_repairs_a_missing_main_member_after_reopen_and_is_idempotent() {
    let mut fixture = RepairFixture::new();
    let outcome = AssertUnwindSafe(Box::pin(async {
        fixture.start().await;
        fixture.prepare().await;
        assertions::before_private_override(&fixture).await;
        fixture.prepare_private_override().await;
        assertions::before_shared_choices(&fixture).await;
        fixture.prepare_shared_choices().await;
        assertions::after_shared_choices(&fixture);
        historical::capture(&mut fixture).await;
        assertions::before_close(&fixture).await;
        fixture.close_initial().await;
        historical::remove_historical_projections(&fixture).await;
        fixture.reopen().await;
        assertions::after_first_reopen(&mut fixture).await;
        fixture.close_reopened().await;
        fixture.reopen().await;
        assertions::after_second_reopen(&fixture).await;
        fixture.close_reopened().await;
    }))
    .catch_unwind()
    .await;
    fixture.finish().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
