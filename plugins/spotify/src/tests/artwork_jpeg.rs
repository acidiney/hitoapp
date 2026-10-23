use super::Server;
use crate::{Config, router};
use axum::{Router, http::StatusCode, response::Html, routing::get};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use image::{ExtendedColorType, codecs::jpeg::JpegEncoder};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const PROVIDER_ID: &str = "0000000000000000000002";

#[tokio::test]
async fn jpeg_without_its_eoi_marker_is_rejected_after_a_valid_jpeg_is_served_exactly() {
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode(&[200, 60, 30].repeat(4), 2, 2, ExtendedColorType::Rgb8)
        .unwrap();
    assert_eq!(&jpeg[jpeg.len() - 2..], &[0xff, 0xd9]);
    let image = Arc::new(Mutex::new(jpeg.clone()));
    let served_image = image.clone();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let embed_calls = calls.clone();
    let image_calls = calls.clone();
    let image_url = Arc::new(OnceLock::<String>::new());
    let embedded_image_url = image_url.clone();
    let upstream = Server::start(
        Router::new()
            .route("/embed/track/0000000000000000000002", get(move || {
                let calls = embed_calls.clone();
                let image_url = embedded_image_url.clone();
                async move {
                    calls.lock().unwrap().push("embed");
                    let bootstrap = json!({
                        "props": {"pageProps": {"state": {"data": {"entity": {
                            "type": "track", "id": PROVIDER_ID,
                            "uri": format!("spotify:track:{PROVIDER_ID}"),
                            "visualIdentity": {"image": [{
                                "url": image_url.get().unwrap(), "maxWidth": 2, "maxHeight": 2
                            }]}
                        }}}}}
                    });
                    Html(format!("<!doctype html><script id=\"__NEXT_DATA__\" type=\"application/json\">{bootstrap}</script>"))
                }
            }))
            .route("/image/{asset}", get(move || {
                let calls = image_calls.clone();
                let image = served_image.clone();
                async move {
                    calls.lock().unwrap().push("image");
                    let bytes = image.lock().unwrap().clone();
                    ([("content-type", "image/jpeg")], bytes)
                }
            })),
    ).await;
    image_url
        .set(
            upstream
                .url
                .join(&format!("image/{:040}", 2))
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
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let request = ArtworkRequest {
        entity: ArtworkEntity::Recording,
        provider_id: PROVIDER_ID.to_owned(),
    };
    let valid = client
        .post(plugin.url.join(ARTWORK_PATH).unwrap())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(valid.status(), StatusCode::OK);
    assert_eq!(valid.headers()["content-type"], "image/jpeg");
    assert_eq!(
        valid.headers()["etag"].to_str().unwrap(),
        format!("\"{:x}\"", Sha256::digest(&jpeg))
    );
    assert_eq!(valid.bytes().await.unwrap().as_ref(), jpeg.as_slice());

    let mut truncated = jpeg.clone();
    truncated.truncate(truncated.len() - 2);
    *image.lock().unwrap() = truncated;
    let invalid = client
        .post(plugin.url.join(ARTWORK_PATH).unwrap())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(
        invalid.status(),
        StatusCode::BAD_GATEWAY,
        "JPEG bytes without the EOI marker must be rejected"
    );
    assert_eq!(
        invalid.json::<serde_json::Value>().await.unwrap(),
        json!({"error": "Spotify returned invalid artwork"})
    );
    assert_eq!(*calls.lock().unwrap(), ["embed", "image", "embed", "image"]);
}
