use super::Server;
use crate::{Config, router};
use axum::{
    Router,
    http::{StatusCode, Uri},
    response::Html,
    routing::get,
};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkEntity, ArtworkRequest};
use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const PROVIDER_ID: &str = "0000000000000000000002";
const OTHER_ID: &str = "0000000000000000000004";
const PNG: &[u8] = include_bytes!("fixtures/artwork.png");

#[derive(Clone, Copy)]
enum Variation {
    Png,
    Jpeg,
    WrongType,
    WrongId,
    WrongUri,
    Truncated,
    LargeDimensions,
    IendCrc,
    TrailingTextCrc,
    Scheme,
    Origin,
    Query,
}

fn encoded_image(variation: Variation) -> Vec<u8> {
    let mut bytes = Vec::new();
    match variation {
        Variation::Jpeg => {
            let pixels = [200, 60, 30].repeat(4);
            JpegEncoder::new_with_quality(&mut bytes, 85)
                .encode(&pixels, 2, 2, ExtendedColorType::Rgb8)
                .unwrap();
            bytes
        }
        Variation::LargeDimensions => {
            let pixels = [40, 150, 210, 255].repeat(1025);
            PngEncoder::new(&mut bytes)
                .write_image(&pixels, 1025, 1, ExtendedColorType::Rgba8)
                .unwrap();
            bytes
        }
        Variation::Truncated => PNG[..20].to_vec(),
        Variation::IendCrc => {
            bytes.extend_from_slice(PNG);
            *bytes.last_mut().unwrap() ^= 1;
            bytes
        }
        Variation::TrailingTextCrc => {
            {
                let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().unwrap();
                writer
                    .write_image_data(&[40, 150, 210, 255].repeat(4))
                    .unwrap();
                writer
                    .write_chunk(png::chunk::tEXt, b"Owned\0Trailing fixture")
                    .unwrap();
                writer.finish().unwrap();
            }
            assert_eq!(&bytes[bytes.len() - 8..bytes.len() - 4], b"IEND");
            let text_crc = bytes.len() - 13;
            bytes[text_crc] ^= 1;
            bytes
        }
        _ => PNG.to_vec(),
    }
}

#[tokio::test]
async fn artwork_validation_binds_entities_and_uses_validated_image_bytes() {
    for (label, entity, variation) in [
        (
            "JPEG with false PNG content type",
            ArtworkEntity::Recording,
            Variation::Jpeg,
        ),
        ("playlist cover", ArtworkEntity::Playlist, Variation::Png),
        ("release cover", ArtworkEntity::Release, Variation::Png),
        (
            "wrong entity type",
            ArtworkEntity::Recording,
            Variation::WrongType,
        ),
        (
            "wrong entity ID",
            ArtworkEntity::Recording,
            Variation::WrongId,
        ),
        (
            "wrong entity URI",
            ArtworkEntity::Recording,
            Variation::WrongUri,
        ),
        (
            "truncated image",
            ArtworkEntity::Recording,
            Variation::Truncated,
        ),
        (
            "oversized decoded dimensions",
            ArtworkEntity::Recording,
            Variation::LargeDimensions,
        ),
        (
            "corrupted IEND CRC",
            ArtworkEntity::Recording,
            Variation::IendCrc,
        ),
        (
            "corrupted post-IDAT text CRC",
            ArtworkEntity::Recording,
            Variation::TrailingTextCrc,
        ),
        (
            "unsupported scheme",
            ArtworkEntity::Recording,
            Variation::Scheme,
        ),
        (
            "unapproved origin",
            ArtworkEntity::Recording,
            Variation::Origin,
        ),
        ("image query", ArtworkEntity::Recording, Variation::Query),
    ] {
        let kind = match entity {
            ArtworkEntity::Artist => "artist",
            ArtworkEntity::Playlist => "playlist",
            ArtworkEntity::Recording => "track",
            ArtworkEntity::Release => "album",
            ArtworkEntity::Featured => {
                unreachable!("the validation table never asks for featured artwork")
            }
        };
        let image = Arc::new(encoded_image(variation));
        let served_image = image.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let embed_calls = calls.clone();
        let image_calls = calls.clone();
        let other_calls = calls.clone();
        let image_url = Arc::new(OnceLock::<String>::new());
        let embedded_image_url = image_url.clone();
        let other_origin = Server::start(Router::new().fallback(move || {
            let calls = other_calls.clone();
            async move {
                calls.lock().unwrap().push("other_origin");
                ([("content-type", "image/png")], PNG)
            }
        }))
        .await;
        let upstream = Server::start(
            Router::new()
                .route(
                    "/embed/{kind}/{id}",
                    get(move |uri: Uri| {
                        let calls = embed_calls.clone();
                        let image_url = embedded_image_url.clone();
                        async move {
                            calls.lock().unwrap().push("embed");
                            assert_eq!(
                                uri.path_and_query().unwrap().as_str(),
                                format!("/embed/{kind}/{PROVIDER_ID}")
                            );
                            let mut metadata = json!({
                                "type": kind,
                                "id": PROVIDER_ID,
                                "uri": format!("spotify:{kind}:{PROVIDER_ID}")
                            });
                            match variation {
                                Variation::WrongType => metadata["type"] = json!("album"),
                                Variation::WrongId => metadata["id"] = json!(OTHER_ID),
                                Variation::WrongUri => metadata["uri"] = json!(format!("spotify:{kind}:{OTHER_ID}")),
                                _ => {}
                            }
                            if entity == ArtworkEntity::Playlist {
                                metadata["coverArt"] = json!({"sources": [{
                                    "url": image_url.get().unwrap(),
                                    "width": 2,
                                    "height": 2
                                }]});
                            } else {
                                metadata["visualIdentity"] = json!({"image": [{
                                    "url": image_url.get().unwrap(),
                                    "maxWidth": 2,
                                    "maxHeight": 2
                                }]});
                            }
                            let bootstrap = json!({
                                "props": {"pageProps": {"state": {"data": {"entity": metadata}}}}
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
                        let image = served_image.clone();
                        async move {
                            calls.lock().unwrap().push("image");
                            ([("content-type", "image/png")], image.as_ref().clone())
                        }
                    }),
                ),
        )
        .await;
        let image_path = format!("image/{:040}", 2);
        let mut location = upstream.url.join(&image_path).unwrap();
        match variation {
            Variation::Scheme => location.set_scheme("ftp").unwrap(),
            Variation::Origin => location = other_origin.url.join(&image_path).unwrap(),
            Variation::Query => location.set_query(Some("owned-unsupported=1")),
            _ => {}
        }
        image_url.set(location.to_string()).unwrap();
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
                entity,
                provider_id: PROVIDER_ID.to_owned(),
            })
            .send()
            .await
            .unwrap();

        let success = matches!(variation, Variation::Png | Variation::Jpeg);
        let expected_status = if success {
            StatusCode::OK
        } else {
            StatusCode::BAD_GATEWAY
        };
        assert_eq!(response.status(), expected_status, "case: {label}");
        if success {
            let expected_type = if matches!(variation, Variation::Jpeg) {
                "image/jpeg"
            } else {
                "image/png"
            };
            assert_eq!(
                response.headers()["content-type"],
                expected_type,
                "case: {label}"
            );
            assert_eq!(
                response.headers()["cache-control"],
                "private, no-store",
                "case: {label}"
            );
            assert_eq!(
                response.headers()["etag"].to_str().unwrap(),
                format!("\"{:x}\"", Sha256::digest(image.as_slice())),
                "case: {label}"
            );
            assert_eq!(
                response.bytes().await.unwrap().as_ref(),
                image.as_slice(),
                "case: {label}"
            );
        } else {
            assert_eq!(
                response.json::<serde_json::Value>().await.unwrap(),
                json!({"error": "Spotify returned invalid artwork"}),
                "case: {label}"
            );
        }
        let expected_calls = if matches!(
            variation,
            Variation::Png
                | Variation::Jpeg
                | Variation::Truncated
                | Variation::LargeDimensions
                | Variation::IendCrc
                | Variation::TrailingTextCrc
        ) {
            vec!["embed", "image"]
        } else {
            vec!["embed"]
        };
        assert_eq!(*calls.lock().unwrap(), expected_calls, "case: {label}");
    }
}
