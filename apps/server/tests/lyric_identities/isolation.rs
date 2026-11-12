use super::setup::Prepared;
use crate::{account, source_provider};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::json;
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn held_lyric_search_and_metadata_do_not_take_ordinary_source_refresh_or_audio_admission() {
    let prepared = Prepared::new().await;
    let mut source = source_provider::Provider::start("lyric-identity-isolation", 0).await;
    let installation = prepared
        .library
        .account
        .connect(
            &prepared.library.app,
            &source.url,
            "Owned ordinary audio",
            None,
        )
        .await;
    let (status,adopted)=prepared.library.account.request(&prepared.library.app,"POST",&format!("/api/v1/plugins/{}/recordings",installation["id"].as_str().unwrap()),json!({"installation_revision":installation["revision"],"provider_recording_id":source.recording_id()})).await;
    assert_eq!(status, StatusCode::OK);
    let sources_path = format!(
        "/api/v1/recordings/{}/sources",
        adopted["id"].as_str().unwrap()
    );
    let before = prepared.library.snapshot().await;
    let mut rejected = None;
    for (call, workflow, body) in [
        ("metadata", "identities/review", prepared.candidate.clone()),
        (
            "search",
            "candidates",
            json!({"installation_id":prepared.installation["id"],"installation_revision":prepared.installation["revision"],"query":"Explicit private search"}),
        ),
    ] {
        source.replace_unavailable();
        source.make_ready();
        let gate = prepared.provider.hold(call);
        let app = prepared.library.app.clone();
        let path = format!("{}/{workflow}", prepared.base());
        let headers = prepared.library.account.headers();
        let task =
            tokio::spawn(
                async move { account::exchange(&app, "POST", &path, body, headers).await },
            );
        tokio::time::timeout(Duration::from_secs(2), gate.entered())
            .await
            .unwrap();
        let (status, refreshed) = prepared
            .library
            .account
            .request(
                &prepared.library.app,
                "POST",
                &format!("{sources_path}/refresh"),
                json!({"purpose":"playback"}),
            )
            .await;
        if status != StatusCode::OK {
            task.abort();
            let _ = task.await;
            tokio::time::timeout(Duration::from_secs(2), gate.retired())
                .await
                .unwrap();
            prepared.provider.release();
            rejected = Some(status);
            break;
        }
        let ready = refreshed
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["ready"] == true)
            .unwrap();
        let mut request = Request::get(format!(
            "/api/v1/sources/{}/audio",
            ready["id"].as_str().unwrap()
        ))
        .body(Body::empty())
        .unwrap();
        *request.headers_mut() = prepared.library.account.headers();
        request
            .headers_mut()
            .insert("range", "bytes=44-107".parse().unwrap());
        tokio::time::timeout(Duration::from_secs(2), async {
            let response = prepared.library.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                response.headers()["content-range"],
                format!("bytes 44-107/{}", source.audio().len())
            );
            assert_eq!(
                to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap()
                    .as_ref(),
                &source.audio()[44..108]
            );
        })
        .await
        .expect(
            "The actual ordinary audio range must finish while lyric identity work remains held",
        );
        assert!(!gate.released() && !task.is_finished());
        prepared.provider.release();
        let (status, _, _) = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, StatusCode::OK);
    }
    let joined = source.finish().await;
    prepared.library.assert_original();
    if rejected.is_none() {
        assert_eq!(source.requests(), 7);
        assert_eq!(source.audio_requests(), 2);
        assert_eq!(source.unexpected(), 0);
        assert_eq!(prepared.library.snapshot().await, before);
        prepared
            .provider
            .assert_calls(&["manifest", "metadata", "metadata", "metadata", "search"]);
    }
    prepared.provider.finish().await;
    assert!(joined);
    assert_eq!(
        rejected, None,
        "A held lyric identity search or metadata review must not consume the ordinary per-account provider admission"
    );
}
