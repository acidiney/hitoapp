use super::Server;
use crate::{Config, router};
use axum::{Router, http::StatusCode, response::Html, routing::get};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use serde_json::json;
use std::{
    io::Cursor,
    sync::{Arc, OnceLock},
    time::Duration,
};

const PROVIDER_ID: &str = "0000000000000000000002";

fn owned_two_frame_png() -> Vec<u8> {
    let first = [255, 0, 0, 255].repeat(4);
    let second = [0, 0, 255, 255].repeat(4);
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&first).unwrap();
        writer.write_image_data(&second).unwrap();
        writer.finish().unwrap();
    }

    let mut reader = png::Decoder::new(Cursor::new(bytes.as_slice()))
        .read_info()
        .unwrap();
    assert_eq!(
        reader.info().animation_control.as_ref().unwrap().num_frames,
        2
    );
    let mut decoded = [0; 16];
    for expected in [first, second] {
        let frame = reader.next_frame(&mut decoded).unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(&decoded[..frame.buffer_size()], expected.as_slice());
    }
    bytes
}

#[tokio::test]
async fn animated_png_is_rejected_with_a_static_artwork_error() {
    let served_image = Arc::new(owned_two_frame_png());
    let image_url = Arc::new(OnceLock::<String>::new());
    let embedded_image_url = image_url.clone();
    let upstream = Server::start(
        Router::new()
            .route(
                "/embed/track/0000000000000000000002",
                get(move || {
                    let image_url = embedded_image_url.clone();
                    async move {
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
                    let image = served_image.clone();
                    async move { ([("content-type", "image/png")], image.as_ref().clone()) }
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
    let response = client
        .post(plugin.url.join(ARTWORK_PATH).unwrap())
        .json(&ArtworkRequest {
            entity: ArtworkEntity::Recording,
            provider_id: PROVIDER_ID.to_owned(),
        })
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        json!({"error": "Spotify returned invalid artwork"})
    );
}
