use super::support;

use hito_debrid_premiumize::{Error, PremiumizeClient, TransferBinding};
use serde_json::{Value, json};
use support::{Fixture, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-premiumize-credential-kept-private";
const TRANSFER_ID: &str = "owned-transfer-kept-private";
const CREATED_AT: u64 = 1_700_000_000;

mod identity;
mod lifecycle;
mod rejections;
mod response;

fn binding() -> TransferBinding {
    TransferBinding {
        transfer_id: TRANSFER_ID.into(),
        created_at_unix_seconds: CREATED_AT,
    }
}

fn transfer(status: &str, progress: f64) -> Value {
    json!({
        "status": "success",
        "transfers": [{
            "id": TRANSFER_ID,
            "created_at": CREATED_AT,
            "status": status,
            "progress": progress,
        }]
    })
}

fn client(fixture: &Fixture) -> PremiumizeClient {
    PremiumizeClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address()).unwrap()
}

#[tokio::test]
async fn documented_fixed_progress_must_agree_with_the_phase() {
    let fixture = Fixture::start(
        [("queued", 0.5), ("error", 0.5), ("finished", 0.5)]
            .into_iter()
            .map(|(status, fraction)| serde_json::to_vec(&transfer(status, fraction)).unwrap())
            .collect(),
    )
    .await;
    let client = client(&fixture);
    for _ in 0..3 {
        assert!(matches!(
            client.observe(&binding()).await,
            Err(Error::InvalidResponse)
        ));
    }
    assert_eq!(fixture.close().await.len(), 3);
}
