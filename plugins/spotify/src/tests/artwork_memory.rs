use super::Server;
use crate::{Config, router};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::{Html, Response},
    routing::get,
};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use http_body_util::BodyExt;
use serde_json::json;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

const PROVIDER_ID: &str = "0000000000000000000002";
const IMAGE: &[u8] = include_bytes!("fixtures/artwork.png");

async fn artwork(app: &Router) -> Response {
    app.clone()
        .oneshot(
            Request::post(format!("/{ARTWORK_PATH}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&ArtworkRequest {
                        entity: ArtworkEntity::Recording,
                        provider_id: PROVIDER_ID.to_owned(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn retained_artwork_frames_hold_admission_until_the_encoded_bytes_are_dropped() {
    let calls = Arc::new(AtomicUsize::new(0));
    let embed_calls = calls.clone();
    let image_calls = calls.clone();
    let image_url = Arc::new(OnceLock::<String>::new());
    let embedded_image_url = image_url.clone();
    let upstream = Server::start(
        Router::new()
            .route(
                "/embed/track/0000000000000000000002",
                get(move || {
                    let calls = embed_calls.clone();
                    let image_url = embedded_image_url.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        let bootstrap = json!({
                            "props": {"pageProps": {"state": {"data": {"entity": {
                                "type": "track",
                                "id": PROVIDER_ID,
                                "uri": format!("spotify:track:{PROVIDER_ID}"),
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
                get(move || {
                    let calls = image_calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
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
    .unwrap();

    let mut retained = Vec::new();
    for _ in 0..4 {
        let response = artwork(&app).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        let frame = body.frame().await.unwrap().unwrap();
        let bytes = frame.into_data().unwrap();
        assert_eq!(bytes.as_ref(), IMAGE);
        drop(body);
        retained.push(bytes);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 8);

    let blocked = artwork(&app).await;
    assert_eq!(blocked.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(calls.load(Ordering::SeqCst), 8);
    drop(blocked);

    drop(retained.remove(0));
    let recovered = artwork(&app).await;
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 10);
    assert_eq!(
        recovered
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        IMAGE
    );
}
