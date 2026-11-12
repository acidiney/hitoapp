use crate::{
    fixture::{AudioFixture, no_configuration},
    tones,
};
use axum::http::StatusCode;
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn body_errors_keep_request_and_account_correlation_without_private_inputs() {
    let fixture = AudioFixture::start("flac", |path| tones::flac(path, 5), no_configuration).await;
    let actor = fixture
        .services
        .identity
        .authenticate(&fixture.token)
        .await
        .unwrap();
    let logs = LogCapture::default();
    let dispatch =
        logs.dispatch_with_spans("off,hito_server=trace,hito_core::playback_access=trace");
    let request_id = async {
        let response = fixture
            .response("GET", "?quality=saver&start_ms=1234")
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        fixture
            .services
            .identity
            .logout(&fixture.token)
            .await
            .unwrap();
        assert!(response.into_body().collect().await.is_err());
        request_id
    }
    .with_subscriber(dispatch)
    .await;
    let events = logs.events();
    let error = events
        .iter()
        .find(|event| event["fields"]["event"] == "audio_quality_body_failed")
        .expect("body failure must have a safe event");
    let spans = error["spans"]
        .as_array()
        .expect("body error must retain its spans");
    assert!(
        spans
            .iter()
            .any(|span| span["name"] == "http_request" && span["request_id"] == request_id)
    );
    assert!(
        spans
            .iter()
            .any(|span| span["account_id"] == actor.user().id)
    );
    assert!(
        events
            .iter()
            .any(|event| event["fields"]["event"] == "audio_quality_response"
                && event["fields"]["outcome"] == "converted")
    );
    logs.assert_excludes(&[
        "Private Quality Title",
        fixture.directory.path().to_str().unwrap(),
        &fixture.token,
        "start_ms",
        "quality=saver",
        "/dev/stdin",
        "pipe:1",
    ]);
}
