use super::Server;
use crate::{Config, router};
use axum::{Router, http::StatusCode, response::Html, routing::get};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use serde_json::json;
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const ARTIST_ID: &str = "0000000000000000000004";
const IMAGE: &[u8] = include_bytes!("fixtures/artwork.png");

struct Fixture {
    plugin: Server,
    _upstream: Server,
    calls: Arc<Mutex<Vec<String>>>,
}

fn bootstrap_page(kind: &str, id: &str, images: serde_json::Value) -> Html<String> {
    let bootstrap = json!({
        "props": {"pageProps": {"state": {"data": {"entity": {
            "id": id,
            "uri": format!("spotify:{kind}:{id}"),
            "type": kind,
            "visualIdentity": {"image": images}
        }}}}}
    });
    Html(format!(
        "<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"
    ))
}

async fn fixture(served_kind: &'static str, image_count: usize) -> Fixture {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let embed_calls = calls.clone();
    let image_calls = calls.clone();
    let image_url = Arc::new(OnceLock::<String>::new());
    let embedded_image_url = image_url.clone();
    let upstream = Server::start(
        Router::new()
            .route(
                "/embed/{kind}/{id}",
                get(
                    move |axum::extract::Path((kind, id)): axum::extract::Path<(String, String)>| {
                        let calls = embed_calls.clone();
                        let image_url = embedded_image_url.clone();
                        async move {
                            calls.lock().unwrap().push(format!("embed:{kind}"));
                            let images = (0..image_count)
                                .map(|_| {
                                    json!({"url": image_url.get().unwrap(), "maxWidth": 2, "maxHeight": 2})
                                })
                                .collect::<Vec<_>>();
                            bootstrap_page(served_kind, &id, json!(images))
                        }
                    },
                ),
            )
            .route(
                "/image/{asset}",
                get(move || {
                    let calls = image_calls.clone();
                    async move {
                        calls.lock().unwrap().push("image".to_owned());
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
                .join(&format!("image/{:040}", 4))
                .unwrap()
                .to_string(),
        )
        .unwrap();
    let plugin = Server::start(
        router(Config {
            public_base: upstream.url.clone(),
            query_endpoint: upstream.url.join("pathfinder/v2/query").unwrap(),
        })
        .unwrap(),
    )
    .await;
    Fixture {
        plugin,
        _upstream: upstream,
        calls,
    }
}

async fn request(fixture: &Fixture, provider_id: &str) -> reqwest::Response {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
        .post(fixture.plugin.url.join(ARTWORK_PATH).unwrap())
        .json(&ArtworkRequest {
            entity: ArtworkEntity::Artist,
            provider_id: provider_id.to_owned(),
        })
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn artist_photo_is_read_from_the_artist_page_and_the_allowed_image_origin() {
    let fixture = fixture("artist", 1).await;
    let response = request(&fixture, ARTIST_ID).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.bytes().await.unwrap().as_ref(), IMAGE);
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed:artist", "image"]);
}

#[tokio::test]
async fn artist_page_that_describes_another_entity_is_refused_before_any_image_request() {
    let fixture = fixture("track", 1).await;
    let response = request(&fixture, ARTIST_ID).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed:artist"]);
}

#[tokio::test]
async fn artist_without_a_photo_is_not_found() {
    let fixture = fixture("artist", 0).await;
    let response = request(&fixture, ARTIST_ID).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(*fixture.calls.lock().unwrap(), ["embed:artist"]);
}

#[tokio::test]
async fn artist_identifiers_that_are_not_spotify_ids_never_reach_spotify() {
    let fixture = fixture("artist", 1).await;
    for id in ["short", "../../../etc/passwd0000", "000000000000000000000!"] {
        assert_eq!(
            request(&fixture, id).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert!(fixture.calls.lock().unwrap().is_empty());
}
