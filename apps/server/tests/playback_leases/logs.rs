use super::fixture::Case;
use axum::http::StatusCode;
use hito_core::PlaybackLease;
use hito_testkit::logs::LogCapture;
use http_body_util::BodyExt;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn interrupted_original_bodies_keep_http_correlation_without_private_material() {
    let f = Case::new().await;
    let logs = LogCapture::default();
    let (request_id, status, interrupted) = async {
        let response = f.begin().await;
        assert_eq!(response.status(), StatusCode::OK);
        let lease: PlaybackLease =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let response = f
            .request(
                "GET",
                &format!("/api/v1/playback/leases/{}/audio", lease.id),
                &[],
                None,
            )
            .await;
        let status = response.status();
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        f.services.identity.logout(&f.token).await.unwrap();
        let interrupted = response.into_body().collect().await.is_err();
        (request_id, status, interrupted)
    }
    .with_subscriber(
        logs.dispatch_with_spans("off,hito_server=trace,hito_core::playback_access=trace"),
    )
    .await;
    let account = f.actor.user().id.clone();
    logs.assert_excludes(&[
        &f.token,
        f.directory.path().to_str().unwrap(),
        "private-owned.wav",
        "owned HTTP lease test password",
    ]);
    f.close().await;
    let events = logs.events();
    let correlated = events.iter().any(|event| {
        event["fields"]["event"] == "audio_original_body_failed"
            && event["fields"]["error_kind"].is_string()
            && event["spans"].as_array().is_some_and(|spans| {
                spans
                    .iter()
                    .any(|span| span["name"] == "http_request" && span["request_id"] == request_id)
                    && spans.iter().any(|span| span["account_id"] == account)
            })
    });
    assert_eq!(status, StatusCode::OK);
    assert!(
        interrupted && correlated,
        "Original audio body failures must retain the GET request and account correlation"
    );
}
