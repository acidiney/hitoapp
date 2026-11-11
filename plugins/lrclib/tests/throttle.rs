mod support;

use axum::http::StatusCode;
use axum::{body::Body, http::Request};
use hito_plugin_lrclib::{Config, router};
use serde_json::json;
use std::time::{Duration, SystemTime};
use support::{Reply, Server, TOKEN, Upstream, authorized, record};
use tower::ServiceExt;
use url::Url;

#[tokio::test]
async fn a_future_http_date_cooldown_does_not_turn_into_an_early_sixty_second_retry() {
    let (upstream, upstream_server) = Upstream::start(record(41)).await;
    let future = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(600));
    *upstream.response.lock().unwrap() =
        Some(Reply::Retry(StatusCode::TOO_MANY_REQUESTS, vec![future]));
    let config = Config::new(TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&upstream_server.url).unwrap())
        .unwrap();
    let service = Server::start(router(config).unwrap()).await;
    let client = reqwest::Client::new();
    let lookup = || {
        authorized(client.post(format!("{}v2/lyrics", service.url)))
            .json(&json!({"recording":{"id":"41","revision":null}}))
    };
    assert_eq!(lookup().send().await.unwrap().status(), 503);
    *upstream.response.lock().unwrap() = None;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::time::resume();
    assert_eq!(lookup().send().await.unwrap().status(), 503);
    assert_eq!(
        upstream.seen.lock().unwrap().len(),
        1,
        "cooldown did not contact the upstream early"
    );
    service.close().await;
    upstream_server.close().await;
}

fn request() -> Request<Body> {
    Request::post("/v2/lyrics")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("x-hito-plugin-protocol", "1")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"recording":{"id":"41","revision":null}}).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn a_rate_limit_cooldown_survives_cancelled_waiters_and_overload_uses_the_same_policy() {
    for status in [
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let (upstream, server) = Upstream::start(record(41)).await;
        *upstream.response.lock().unwrap() = Some(Reply::Retry(status, vec!["2".into()]));
        let app = router(
            Config::new(TOKEN)
                .unwrap()
                .with_endpoint(Url::parse(&server.url).unwrap())
                .unwrap(),
        )
        .unwrap();
        let started = tokio::time::Instant::now();
        assert_eq!(app.clone().oneshot(request()).await.unwrap().status(), 503);
        *upstream.response.lock().unwrap() = None;
        {
            let pending = app.clone().oneshot(request());
            tokio::pin!(pending);
            tokio::select! {
                _ = &mut pending => panic!("cooldown waiter completed early"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {},
            }
        }
        assert_eq!(upstream.seen.lock().unwrap().len(), 1);
        assert_eq!(app.oneshot(request()).await.unwrap().status(), 200);
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert_eq!(upstream.seen.lock().unwrap().len(), 2);
        server.close().await;
    }
}
