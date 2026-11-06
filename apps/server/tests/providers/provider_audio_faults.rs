use audio_fixture::AudioLibrary;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use hito_core::test_support::provider_audio as audio_fixture;
use http_body_util::BodyExt;
use std::time::{Duration, Instant};
use tower::ServiceExt;

struct Browser {
    app: Router,
    token: String,
    source: String,
}

impl Browser {
    async fn sign_in(f: &AudioLibrary) -> Self {
        let session = f
            .hito
            .identity
            .login(hito_core::Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        Self {
            app: hito_server::router(f.hito.clone()),
            token: session.token,
            source: f.source.id.clone(),
        }
    }

    async fn get(&self, range: Option<&str>) -> Response {
        let mut request = Request::builder()
            .uri(format!("/api/v1/sources/{}/audio", self.source))
            .header("authorization", format!("Bearer {}", self.token));
        if let Some(range) = range {
            request = request.header("range", range);
        }
        self.app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn expired_links_and_provider_errors_reach_the_browser_as_private_gateway_errors_then_recover()
 {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let browser = Browser::sign_in(&f).await;
    let failures = [
        StatusCode::FORBIDDEN,
        StatusCode::GONE,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::SERVICE_UNAVAILABLE,
    ];
    f.server.audio_statuses(failures.to_vec());
    for status in failures {
        let started = Instant::now();
        let response = browser.get(None).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{status}");
        assert!(started.elapsed() < Duration::from_secs(2), "{status}");
        assert!(response.headers().get("content-range").is_none());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("private-audio-token"), "{text}");
        assert!(!text.contains(&f.server.url), "{text}");
    }
    assert_eq!(
        f.server.requests("v1/audio").await.len(),
        failures.len(),
        "every failed browser request must cost exactly one provider request"
    );
    let healed = browser.get(None).await;
    assert_eq!(healed.status(), StatusCode::OK);
    assert_eq!(
        healed.into_body().collect().await.unwrap().to_bytes(),
        f.original.as_slice()
    );
}

#[tokio::test]
async fn a_silent_provider_answers_the_browser_with_a_gateway_error_at_the_header_bound() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let browser = Browser::sign_in(&f).await;
    let gate = f.server.hold("v1/audio").await;
    let started = Instant::now();
    let response = browser.get(None).await;
    let waited = started.elapsed();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(
        (Duration::from_secs(9)..Duration::from_secs(13)).contains(&waited),
        "{waited:?}"
    );
    gate.open();
    assert_eq!(browser.get(None).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_provider_that_ignores_a_seek_is_answered_with_the_whole_audio_and_no_range_claim() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server.audio_ignores_ranges();
    let browser = Browser::sign_in(&f).await;
    let response = browser.get(Some("bytes=8000-")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("content-range").is_none());
    assert_eq!(
        response.headers()["content-length"],
        f.original.len().to_string()
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        f.original.as_slice()
    );
}

#[tokio::test]
async fn a_connection_reset_mid_body_fails_the_browser_read_promptly_and_the_next_request_recovers()
{
    let f = AudioLibrary::start().await;
    f.ready().await;
    let browser = Browser::sign_in(&f).await;
    let gate = f.server.hold_broken_tail(64).await;
    let response = browser.get(None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut received = Vec::new();
    while received.len() < 64 {
        let frame = body.frame().await.unwrap().unwrap();
        received.extend_from_slice(&frame.into_data().unwrap());
    }
    assert_eq!(received, &f.original[..64]);
    gate.open();
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match body.frame().await {
                Some(Ok(_)) => {}
                other => break other,
            }
        }
    })
    .await
    .expect("a reset must end the browser read promptly");
    assert!(
        outcome.is_some_and(|frame| frame.is_err()),
        "the browser must see an error, not a clean end of audio"
    );
    f.server.clear_audio_tail_fault().await;
    let recovered = browser.get(None).await;
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(
        recovered.into_body().collect().await.unwrap().to_bytes(),
        f.original.as_slice()
    );
}

#[tokio::test]
async fn a_provider_below_the_throughput_floor_fails_the_browser_read_within_the_window_and_frees_the_slot()
 {
    let f = AudioLibrary::start_paced(hito_core::StreamPace {
        window: Duration::from_millis(1800),
        floor_bytes_per_second: 6000,
        ..hito_core::StreamPace::DEFAULT
    })
    .await;
    f.ready().await;
    f.server
        .audio_trickle(f.original.len() / 6 + 1, Duration::from_millis(900));
    let browser = Browser::sign_in(&f).await;
    let response = browser.get(None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let started = Instant::now();
    let mut body = response.into_body();
    let mut received = 0;
    let failed = loop {
        match body.frame().await {
            Some(Ok(frame)) => received += frame.into_data().map_or(0, |data| data.len()),
            Some(Err(_)) => break true,
            None => break false,
        }
    };
    assert!(failed, "a stream below the floor must not end cleanly");
    assert!(
        started.elapsed() < Duration::from_millis(3600),
        "{:?}",
        started.elapsed()
    );
    assert!(received < f.original.len());
    f.server.audio_trickle(f.original.len(), Duration::ZERO);
    for _ in 0..2 {
        let healed = browser.get(None).await;
        assert_eq!(healed.status(), StatusCode::OK);
        assert_eq!(
            healed.into_body().collect().await.unwrap().to_bytes(),
            f.original.as_slice()
        );
    }
}
