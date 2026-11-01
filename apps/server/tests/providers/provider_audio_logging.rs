use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::provider_audio::AudioLibrary;
use http_body_util::BodyExt;
use serde_json::Value;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_failed_provider_audio_fetch_logs_one_correlated_warning_and_no_error() {
    hito_testkit::logs::anchor_callsite_interest();
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        let f = AudioLibrary::start().await;
        f.server
            .audio(
                StatusCode::BAD_GATEWAY,
                Default::default(),
                b"provider-private-upstream-body".to_vec(),
            )
            .await;
        let session = f
            .hito
            .identity
            .login(hito_core::Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let app = hito_server::router(f.hito.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sources/{}/audio", f.source.id))
                    .header("authorization", format!("Bearer {}", session.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        response.into_body().collect().await.unwrap();
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let events: Vec<Value> = output
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let loud: Vec<&Value> = events
            .iter()
            .filter(|event| matches!(event["level"].as_str(), Some("ERROR" | "WARN")))
            .collect();
        assert_eq!(loud.len(), 1, "one failed fetch must log once: {loud:#?}");
        let event = loud[0];
        assert_eq!(event["level"], "WARN");
        assert_eq!(event["fields"]["event"], "upstream_failed");
        assert_eq!(event["fields"]["error_kind"], "upstream");
        assert!(
            event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|span| span["request_id"] == request_id.as_str()),
            "the warning must carry the request correlation"
        );
        assert!(!output.contains("provider-private-"));
        assert!(!output.contains("private-audio-token"));
        assert!(!output.contains(&session.token));
    }
    .with_subscriber(subscriber)
    .await;
}
