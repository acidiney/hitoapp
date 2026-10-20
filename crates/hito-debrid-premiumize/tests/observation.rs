mod support;

use hito_debrid_premiumize::{Error, PremiumizeClient, TransferBinding, TransferPhase};
use serde_json::json;
use support::{Fixture, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-premiumize-credential-kept-private";
const TRANSFER_ID: &str = "owned-transfer-kept-private";
const CREATED_AT: u64 = 1_700_000_000;

#[tokio::test]
async fn exact_transfer_retains_its_binding_through_all_phases_and_rejects_replacement() {
    let configured = TransferBinding {
        transfer_id: TRANSFER_ID.into(),
        created_at_unix_seconds: CREATED_AT,
    };
    let snapshot = serde_json::to_vec(&configured).expect("the configured binding must serialize");
    let binding: TransferBinding =
        serde_json::from_slice(&snapshot).expect("the configured binding must reopen");
    let fixture = Fixture::start(vec![
        transfer("queued", 0.0, CREATED_AT),
        transfer("running", 1.0, CREATED_AT),
        transfer("error", 0.0, CREATED_AT),
        transfer("seeding", 1.0, CREATED_AT),
        transfer("finished", 1.0, CREATED_AT),
        transfer("finished", 1.0, CREATED_AT + 1),
    ])
    .await;
    let client = PremiumizeClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let subscriber = capture.dispatch();
    let mut results = Vec::new();
    for _ in 0..6 {
        results.push(
            client
                .observe(&binding)
                .with_subscriber(subscriber.clone())
                .await,
        );
    }
    let requests = fixture.close().await;
    let expected = [
        (TransferPhase::Queued, 0.0),
        (TransferPhase::Running, 1.0),
        (TransferPhase::ProviderFailed, 0.0),
        (TransferPhase::ProviderSeeding, 1.0),
        (TransferPhase::ProviderCompleted, 1.0),
    ];
    for (result, (phase, fraction)) in results.drain(..5).zip(expected) {
        let observation = result.expect("the exact transfer must report its documented phase");
        assert!(observation.binding == configured);
        assert_eq!(observation.phase, phase);
        assert_eq!(observation.progress.fraction(), fraction);
    }
    assert!(matches!(results.pop(), Some(Err(Error::TransferChanged))));
    assert!(binding == configured);
    assert!(serde_json::to_vec(&binding).unwrap() == snapshot);
    assert_eq!(requests.len(), 6);
    for request in requests {
        assert!(request.server_name == "www.premiumize.me");
        assert!(request.method == "GET");
        assert!(request.target == "/api/transfer/list");
        assert!(request.header("host") == Some("www.premiumize.me"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("accept") == Some("application/json"));
        assert!(request.header("accept-encoding") == Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("referer").is_none());
        assert!(request.header("content-type").is_none());
        assert!(request.header("apikey").is_none());
        assert!(request.header("pin").is_none());
        assert!(request.body.is_empty());
    }
    capture.assert_workflows(
        &[
            ("queued", 1),
            ("running", 1),
            ("provider_failed", 1),
            ("provider_seeding", 1),
            ("provider_completed", 1),
            ("transfer_changed", 0),
        ],
        &[
            TOKEN,
            TRANSFER_ID,
            "owned-same-name-decoy",
            "owned private transfer name",
            "owned private provider status",
            "owned-private-folder",
            "owned-private-source",
        ],
    );
}

fn transfer(status: &str, progress: f64, created_at: u64) -> Vec<u8> {
    let completed = matches!(status, "seeding" | "finished");
    serde_json::to_vec(&json!({
        "status": "success",
        "transfers": [
            {
                "id": "owned-same-name-decoy",
                "name": "owned private transfer name",
                "status": "finished",
                "progress": 1.0,
                "message": "owned private provider status",
                "folder_id": "owned-private-folder",
                "file_id": null,
                "created_at": CREATED_AT
            },
            {
                "id": TRANSFER_ID,
                "name": "owned private transfer name",
                "status": status,
                "progress": progress,
                "message": "owned private provider status",
                "folder_id": completed.then_some("owned-private-folder"),
                "file_id": null,
                "created_at": created_at,
                "src": "https://www.premiumize.me/api/job/src?id=owned-private-source",
                "future_provider_field": "compatible extension"
            }
        ]
    }))
    .expect("the owned provider response must serialize")
}

#[path = "protocol/mod.rs"]
mod protocol;
