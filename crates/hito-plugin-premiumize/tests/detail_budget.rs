#![cfg(feature = "test-support")]

#[path = "qualification/mod.rs"]
pub mod qualification;

use hito_plugin_premiumize::router;
use hito_plugin_sdk::{ProviderRecording, RecordingAttribution, RecordingRequest};
use qualification::{
    headers,
    provider::{RECORDING, SERVICE, recording},
    run, transfer,
};
use reqwest::{Client, StatusCode};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn exact_core_detail_budget_is_served_and_one_extra_byte_is_rejected_at_startup() {
    run(vec![transfer("queued", 0.0)], |case| {
        Box::pin(async move {
            let boundary = recording_bytes(32_768);
            let mut config = case.config();
            config.connections[0].recordings[0].recording = boundary.clone();
            let base = case.start(config).await;
            let caller = Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(Duration::from_secs(4))
                .build()
                .unwrap();
            let mut response = caller
                .post(base.join("v1/recordings/detail").unwrap())
                .headers(headers(Some(SERVICE)))
                .json(&RecordingRequest { recording_id: RECORDING.into() })
                .send()
                .await
                .expect("the exact-boundary detail request must complete");
            case.record_response(&response, StatusCode::OK);
            assert_eq!(response.content_length(), Some(32_768));
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.expect("detail bytes must complete") {
                assert!(chunk.len() <= 32_768_usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk);
            }
            assert_eq!(bytes.len(), 32_768);
            let actual: ProviderRecording = serde_json::from_slice(&bytes).unwrap();
            actual.validate().unwrap();
            assert!(actual == boundary, "the boundary response changed the typed recording");
            assert_eq!(case.counts(), [0, 0, 0, 0]);
            println!("{}", json!({"event":"owned_detail_budget_boundary_served",
                "served_bytes":bytes.len(), "provider_requests":0}));

            let oversized = recording_bytes(32_769);
            let mut config = case.config();
            config.connections[0].recordings[0].recording = oversized;
            assert!(router(config).is_err(),
                "an SDK-valid 32769-byte recording exceeds Core's detail transport budget and must fail startup");
            assert_eq!(case.counts(), [0, 0, 0, 0]);
        })
    })
    .await;
}

fn recording_bytes(bytes: usize) -> ProviderRecording {
    let mut value = recording(false);
    value.attribution = Some(RecordingAttribution {
        creators: vec!["a".repeat(2048); 16],
        provider_label: "owned-private-detail-budget-attribution".into(),
        content_url: "https://example.invalid/owned-public-attribution".into(),
    });
    let encoded = serde_json::to_vec(&value).unwrap().len();
    assert!(encoded > bytes && encoded - bytes < 2048);
    value.attribution.as_mut().unwrap().creators[15].truncate(2048 - (encoded - bytes));
    value
        .validate()
        .expect("the owned boundary recording must satisfy the SDK");
    assert_eq!(serde_json::to_vec(&value).unwrap().len(), bytes);
    value
}
