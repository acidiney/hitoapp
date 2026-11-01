use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use hito_core::test_support::provider_audio::AudioLibrary;
use std::time::Duration;
use tower::ServiceExt;

fn fetch(app: &Router, source: &str, token: &str) -> impl Future<Output = Response> + use<> {
    let request = app.clone().oneshot(
        Request::builder()
            .uri(format!("/api/v1/sources/{source}/audio"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    );
    async { request.await.unwrap() }
}

#[tokio::test]
async fn a_seek_waits_for_an_abandoned_stream_but_a_third_live_stream_is_refused() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let app = hito_server::router(f.hito.clone());
    let session = f
        .hito
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let source = f.source.id.as_str();
    let token = session.token.as_str();
    let mut playing = fetch(&app, source, token).await;
    let preloaded = fetch(&app, source, token).await;
    assert_eq!(playing.status(), StatusCode::OK);
    assert_eq!(preloaded.status(), StatusCode::OK);
    let seek = tokio::spawn(fetch(&app, source, token));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!seek.is_finished());
    drop(std::mem::take(playing.body_mut()));
    let seek = seek.await.unwrap();
    assert_eq!(seek.status(), StatusCode::OK);
    assert_eq!(
        fetch(&app, source, token).await.status(),
        StatusCode::CONFLICT
    );
}
