#[allow(dead_code)]
#[path = "support/boundary.rs"]
mod boundary;
#[allow(dead_code)]
mod support;

use boundary::{Fixture, private_log_canaries};
use futures_util::FutureExt;
use hito_debrid_real_debrid::{MediaAllowlist, test_support::owned_protocol_client};
use hito_plugin_real_debrid::{Config, CuratedAudioRecording, InitError};
use hito_plugin_sdk::{ProviderRecording, RecordingAttribution, RecordingRequest};
use reqwest::{Method, StatusCode};
use std::{panic::AssertUnwindSafe, time::Duration};
use support::{
    LABEL, MEDIA_HOST, PROVIDER_TOKEN, RECORDING, SERVICE_TOKEN, SOURCE, provider::Counts,
};

#[tokio::test]
async fn exact_core_detail_budget_is_served_and_one_extra_byte_is_rejected_at_startup() {
    let recording = recording_bytes(32_768);
    let fixture = Fixture::start(vec![curated(recording.clone())]).await;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
        exact_detail(&fixture, &recording).await;
        assert_eq!(fixture.provider.counts(), zero_counts());
        let client = owned_protocol_client(
            PROVIDER_TOKEN.into(),
            fixture.provider.ca_der(),
            fixture.provider.api_address,
            fixture.provider.media_address,
        )
        .expect("the owned protocol client must initialize without provider requests");
        let admission = Config::new(
            SERVICE_TOKEN.into(),
            client,
            MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            vec![curated(recording_bytes(32_769))],
        );
        let admission_outcome = match admission {
            Err(InitError::InvalidCatalog) => "invalid_catalog",
            Err(_) => "other_rejection",
            Ok(_) => "accepted",
        };
        assert_eq!(fixture.provider.counts(), zero_counts());
        admission_outcome
    }))
    .catch_unwind()
    .await;
    let (capture, exchanges) = fixture.close().await;
    let admission_outcome = match outcome {
        Ok(result) => result.expect("the owned detail-budget workflow exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        exchanges.is_empty(),
        "metadata admission must not contact API or media peers"
    );
    capture.assert_private_requests(&private_log_canaries(), &[200]);
    eprintln!(
        "detail_budget_evidence served_bytes=32768 overlimit_bytes=32769 admission_outcome={admission_outcome} provider_observations=0 provider_posts=0 media_requests=0 joined=true"
    );
    assert_eq!(
        admission_outcome, "invalid_catalog",
        "an SDK-valid 32769-byte recording must fail catalog admission before HTTP or provider work"
    );
}

async fn exact_detail(fixture: &Fixture, expected: &ProviderRecording) {
    let mut response = fixture
        .request(Method::POST, "v1/recordings/detail")
        .header("x-hito-plugin-protocol", "1")
        .json(&RecordingRequest {
            recording_id: RECORDING.into(),
        })
        .send()
        .await
        .expect("the authenticated exact-boundary detail request must complete");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    for header in ["location", "set-cookie", "content-disposition", "etag"] {
        assert!(
            response.headers().get(header).is_none(),
            "private headers must remain absent"
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.expect("detail bytes must reach EOF") {
        assert!(
            chunk.len() <= 32_768_usize.saturating_sub(bytes.len()),
            "detail exceeded Core's byte budget"
        );
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes.len(), 32_768);
    assert!(
        bytes == serde_json::to_vec(expected).unwrap(),
        "detail must preserve the exact typed recording bytes"
    );
    let actual: ProviderRecording = serde_json::from_slice(&bytes).unwrap();
    actual
        .validate()
        .expect("the received recording must satisfy the SDK");
    assert!(actual == *expected, "the boundary recording changed");
    let text = std::str::from_utf8(&bytes).unwrap();
    for forbidden in [
        PROVIDER_TOKEN,
        SERVICE_TOKEN,
        support::WRONG_TOKEN,
        support::COOKIE,
        support::TORRENT,
        support::HASH,
        support::FILE,
        support::HOSTER_LINK,
        support::MEDIA_TARGET,
        support::EXPECTED_FORM,
    ] {
        assert!(
            !text.contains(forbidden),
            "metadata exposed credentials or a private provider binding"
        );
    }
}

fn recording_bytes(bytes: usize) -> ProviderRecording {
    let mut value = support::recording();
    value.attribution = Some(Box::new(RecordingAttribution {
        creators: vec!["a".repeat(2048); 16],
        provider_label: "owned-private-detail-budget-attribution".into(),
        content_url: "https://example.invalid/owned-public-attribution".into(),
    }));
    let encoded = serde_json::to_vec(&value).unwrap().len();
    assert!(encoded > bytes && encoded - bytes < 2048);
    value.attribution.as_mut().unwrap().creators[15].truncate(2048 - (encoded - bytes));
    value
        .validate()
        .expect("the boundary recording must satisfy every SDK field limit");
    assert_eq!(serde_json::to_vec(&value).unwrap().len(), bytes);
    value
}

fn curated(recording: ProviderRecording) -> CuratedAudioRecording {
    CuratedAudioRecording {
        recording,
        source_id: SOURCE.into(),
        label: LABEL.into(),
        binding: support::binding(),
        audio_kind: hito_debrid_real_debrid::AudioKind::Pcm16Wav,
    }
}

fn zero_counts() -> Counts {
    Counts {
        observations: 0,
        posts: 0,
        media: 0,
    }
}
