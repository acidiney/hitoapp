use super::Server;
use crate::{Config, router};
use axum::{
    Router,
    http::{HeaderMap, StatusCode, Uri},
    response::Html,
    routing::get,
};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use hito_testkit::logs::LogCapture;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const PROVIDER_ID: &str = "0000000000000000000002";
const EMBED_PATH: &str = "/embed/track/0000000000000000000002";
const IMAGE: &[u8] = include_bytes!("fixtures/artwork.png");

#[tokio::test]
async fn artwork_returns_owned_raster_without_forwarding_credentials_or_exposing_metadata() {
    let logs = LogCapture::unbounded();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.target() == "hito_plugin_spotify"
                || metadata.target().starts_with("hito_plugin_spotify::")
        }))
        .with(tracing_subscriber::filter::LevelFilter::TRACE)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(move || captured.clone()),
        );
    let dispatch = tracing::Dispatch::new(subscriber);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let embed_calls = calls.clone();
    let image_calls = calls.clone();
    let image_url = Arc::new(OnceLock::<String>::new());
    let embedded_image_url = image_url.clone();
    let upstream = Server::start(
        Router::new()
            .route(
                EMBED_PATH,
                get(move |headers: HeaderMap, uri: Uri| {
                    let calls = embed_calls.clone();
                    let image_url = embedded_image_url.clone();
                    async move {
                        assert_eq!(uri.path_and_query().unwrap().as_str(), EMBED_PATH);
                        assert!(headers.get("authorization").is_none());
                        assert!(headers.get("cookie").is_none());
                        assert!(headers.get("x-owned-private").is_none());
                        calls.lock().unwrap().push("embed");
                        let bootstrap = json!({
                            "props": {"pageProps": {"state": {"data": {"entity": {
                                "id": PROVIDER_ID,
                                "uri": format!("spotify:track:{PROVIDER_ID}"),
                                "type": "track",
                                "title": "private-artwork-title-sentinel",
                                "artists": [{
                                    "name": "private-artwork-artist-sentinel",
                                    "uri": "spotify:artist:0000000000000000000003"
                                }],
                                "visualIdentity": {"image": [{
                                    "url": image_url.get().unwrap(),
                                    "maxWidth": 2,
                                    "maxHeight": 2
                                }]}
                            }}}}}
                        });
                        Html(format!(
                            "<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"
                        ))
                    }
                }),
            )
            .route(
                "/image/{asset}",
                get(move |headers: HeaderMap, uri: Uri| {
                    let calls = image_calls.clone();
                    async move {
                        assert_eq!(
                            uri.path_and_query().unwrap().as_str(),
                            format!("/image/{:040}", 2)
                        );
                        assert!(headers.get("authorization").is_none());
                        assert!(headers.get("cookie").is_none());
                        assert!(headers.get("x-owned-private").is_none());
                        calls.lock().unwrap().push("image");
                        ([("content-type", "image/png")], IMAGE)
                    }
                }),
            ),
    )
    .await;
    image_url
        .set(
            upstream
                .url
                .join(&format!("image/{:040}", 2))
                .unwrap()
                .to_string(),
        )
        .unwrap();
    let app = router(Config {
        public_base: upstream.url.clone(),
        query_endpoint: upstream.url.join("pathfinder/v2/query").unwrap(),
    })
    .unwrap()
    .layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let dispatch = dispatch.clone();
            async move { next.run(request).with_subscriber(dispatch).await }
        },
    ));
    let plugin = Server::start(app).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let mut endpoint = plugin.url.join(ARTWORK_PATH).unwrap();
    endpoint.set_query(Some(
        "private-artwork-query-sentinel=private-value-sentinel",
    ));
    let response = client
        .post(endpoint)
        .bearer_auth("private-artwork-bearer-sentinel")
        .header("cookie", "private-artwork-cookie-sentinel=private-value")
        .header("x-owned-private", "private-artwork-header-sentinel")
        .json(&ArtworkRequest {
            entity: ArtworkEntity::Recording,
            provider_id: PROVIDER_ID.to_owned(),
        })
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.headers()["etag"].to_str().unwrap(),
        format!("\"{:x}\"", Sha256::digest(IMAGE))
    );
    assert_eq!(response.content_length(), Some(IMAGE.len() as u64));
    for value in response.headers().values() {
        let value = value.to_str().unwrap();
        assert!(!value.contains(image_url.get().unwrap().as_str()));
        assert!(!value.contains("private-"));
    }
    assert_eq!(response.bytes().await.unwrap().as_ref(), IMAGE);
    assert_eq!(*calls.lock().unwrap(), ["embed", "image"]);

    let output = logs.text();
    assert!(!output.contains("private-"));
    assert!(!output.contains(image_url.get().unwrap().as_str()));
    assert!(!output.contains(PROVIDER_ID));
    assert!(!output.contains("spotify:artist:"));
    assert!(!output.contains(upstream.url.as_str()));
    let entries: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(entries.iter().any(|entry| {
        entry["fields"]["event"] == "spotify_request_complete"
            && entry["fields"]["status"] == 200
            && entry["fields"]["elapsed_ms"].is_number()
    }));
}
