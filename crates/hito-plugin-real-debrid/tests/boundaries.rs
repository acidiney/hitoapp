#[path = "support/boundary.rs"]
mod boundary;
#[allow(dead_code)]
mod support;

use boundary::{CHANGED_FILE, Fixture, private_log_canaries, rejected, response};
use futures_util::FutureExt;
use hito_plugin_real_debrid::CuratedWavRecording;
use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};
use support::{LABEL, QUERY, RECORDING, SOURCE};

#[tokio::test]
async fn changed_provider_file_is_revalidated_before_audio_unrestriction() {
    let fixture = Fixture::start(vec![curated(support::recording(), SOURCE)]).await;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
        let bytes = response(
            fixture
                .request(Method::POST, "v1/sources")
                .header("x-hito-plugin-protocol", "1")
                .json(&json!({"recording_id": RECORDING})),
            StatusCode::OK,
        )
        .await;
        let sources: SourcePage =
            serde_json::from_slice(&bytes).expect("sources must use SDK wire data");
        sources.validate().expect("curated sources must validate");
        assert_eq!(sources.items.len(), 1);
        assert_eq!(sources.items[0].id, SOURCE);
        assert!(sources.items[0].ready);
        let bytes = response(
            fixture
                .request(Method::HEAD, "v1/audio?source_id=curated-wav-source-1")
                .header("x-hito-plugin-protocol", "1"),
            StatusCode::OK,
        )
        .await;
        assert!(bytes.is_empty(), "HEAD must not deliver audio");
        assert_eq!(
            fixture.provider.counts(),
            support::provider::Counts {
                observations: 2,
                posts: 0,
                media: 0,
            }
        );

        fixture.provider.replace_selected_file_path(CHANGED_FILE);

        rejected(
            fixture
                .request(Method::GET, "v1/audio?source_id=curated-wav-source-1")
                .header("x-hito-plugin-protocol", "1"),
            StatusCode::BAD_GATEWAY,
            "source_changed",
        )
        .await;
        rejected(
            fixture
                .request(Method::POST, "v1/sources")
                .header("x-hito-plugin-protocol", "1")
                .json(&json!({"recording_id": RECORDING})),
            StatusCode::BAD_GATEWAY,
            "source_changed",
        )
        .await;
        assert_eq!(
            fixture.provider.counts(),
            support::provider::Counts {
                observations: 4,
                posts: 0,
                media: 0,
            }
        );
    }))
    .catch_unwind()
    .await;
    let (capture, exchanges) = fixture.close().await;
    match outcome {
        Ok(result) => result.expect("changed-binding tracer must finish within its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    assert_eq!(
        exchanges.len(),
        4,
        "each explicit check must make exactly one observation"
    );
    for exchange in exchanges {
        assert!(exchange.origin == support::provider::Origin::Api);
        assert!(exchange.method == "GET");
        assert!(exchange.target == format!("/rest/1.0/torrents/info/{}", support::TORRENT));
        assert!(exchange.body.is_empty());
    }
    capture.assert_private_requests(&private_log_canaries(), &[200, 200, 502, 502]);
}

#[tokio::test]
async fn rejected_control_requests_and_catalog_pagination_do_not_contact_the_provider() {
    let first = support::recording();
    let second = ProviderRecording {
        id: "curated-recording-2".into(),
        title: format!("{} second", first.title),
        ..first.clone()
    };
    let fixture = Fixture::start(vec![
        curated(second.clone(), "curated-wav-source-2"),
        curated(first.clone(), SOURCE),
    ])
    .await;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
        reject_invalid_requests(&fixture).await;
        walk_catalog(&fixture, &first, &second).await;
        assert_eq!(
            fixture.provider.counts(),
            support::provider::Counts {
                observations: 0,
                posts: 0,
                media: 0,
            }
        );
    }))
    .catch_unwind()
    .await;
    let (capture, exchanges) = fixture.close().await;
    assert!(
        exchanges.is_empty(),
        "control requests must never reach upstream transport"
    );
    match outcome {
        Ok(result) => result.expect("control tracer must finish within its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    capture.assert_private_requests(
        &private_log_canaries(),
        &[
            400, 400, 400, 400, 400, 400, 400, 404, 404, 404, 404, 200, 200,
        ],
    );
}

fn curated(recording: ProviderRecording, source_id: &str) -> CuratedWavRecording {
    CuratedWavRecording {
        recording,
        source_id: source_id.into(),
        label: LABEL.into(),
        binding: support::binding(),
    }
}

async fn reject_invalid_requests(fixture: &Fixture) {
    rejected(
        fixture
            .request(Method::POST, "v1/search")
            .header("x-hito-plugin-protocol", "1")
            .header("content-type", "application/json")
            .body("{\"query\":\"private-malformed-body-canary\""),
        StatusCode::BAD_REQUEST,
        "invalid_request",
    )
    .await;
    for (path, payload) in [
        (
            "v1/search",
            json!({"query": QUERY, "cursor": null, "limit": 1, "unknown": "private-malformed-body-canary"}),
        ),
        (
            "v1/sources",
            json!({"recording_id": RECORDING, "unknown": "private-malformed-body-canary"}),
        ),
    ] {
        rejected(
            fixture
                .request(Method::POST, path)
                .header("x-hito-plugin-protocol", "1")
                .json(&payload),
            StatusCode::BAD_REQUEST,
            "invalid_request",
        )
        .await;
    }
    let selection = json!({"recording_id": RECORDING});
    for protocol in [None, Some("2"), Some("1")] {
        let mut request = fixture.request(Method::POST, "v1/sources").json(&selection);
        if let Some(protocol) = protocol {
            request = request.header("x-hito-plugin-protocol", protocol);
        }
        if protocol == Some("1") {
            request = request.header("x-hito-plugin-protocol", "1");
        }
        rejected(request, StatusCode::BAD_REQUEST, "invalid_request").await;
    }
    rejected(
        fixture
            .request(
                Method::GET,
                "v1/audio?source_id=curated-wav-source-1&unknown=private-malformed-body-canary",
            )
            .header("x-hito-plugin-protocol", "1"),
        StatusCode::BAD_REQUEST,
        "invalid_request",
    )
    .await;
    for path in ["v1/recordings/detail", "v1/sources"] {
        rejected(
            fixture
                .request(Method::POST, path)
                .header("x-hito-plugin-protocol", "1")
                .json(&json!({"recording_id": "private-unknown-recording-canary"})),
            StatusCode::NOT_FOUND,
            "not_found",
        )
        .await;
    }
    rejected(
        fixture
            .request(
                Method::GET,
                "v1/audio?source_id=private-unknown-source-canary",
            )
            .header("x-hito-plugin-protocol", "1"),
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
    let bytes = response(
        fixture
            .request(
                Method::HEAD,
                "v1/audio?source_id=private-unknown-source-canary",
            )
            .header("x-hito-plugin-protocol", "1"),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert!(
        bytes.is_empty(),
        "rejected HEAD must retain HTTP body semantics"
    );
}

async fn walk_catalog(fixture: &Fixture, first: &ProviderRecording, second: &ProviderRecording) {
    let mut cursor = None;
    for (index, expected) in [first, second].into_iter().enumerate() {
        let bytes = response(
            fixture
                .request(Method::POST, "v1/search")
                .header("x-hito-plugin-protocol", "1")
                .json(&json!({"query": QUERY, "cursor": cursor, "limit": 1})),
            StatusCode::OK,
        )
        .await;
        let page: SearchPage = serde_json::from_slice(&bytes).expect("page must use SDK wire data");
        page.validate(1)
            .expect("cursor page must satisfy SDK bounds");
        assert!(
            page.items.as_slice() == std::slice::from_ref(expected),
            "cursor walk must retain exact catalog data"
        );
        assert_eq!(page.next_cursor.is_some(), index == 0);
        cursor = page.next_cursor;
    }
}
