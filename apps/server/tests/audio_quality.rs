#[path = "audio_quality/fallback.rs"]
mod fallback;
#[path = "fixtures/audio_quality.rs"]
mod fixture;
#[path = "audio_quality/input.rs"]
mod input;
#[path = "audio_quality/lifecycle.rs"]
mod lifecycle;
#[path = "audio_quality/logs.rs"]
mod logs;
#[path = "audio_quality/remote.rs"]
mod remote;
#[path = "audio_quality/representation.rs"]
mod representation;

#[path = "../../../crates/core/tests/fixtures/tone_files.rs"]
#[allow(dead_code)]
mod tones;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use hito_core::{Credentials, Search, test_support::support};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn saver_audio_is_aac_from_the_requested_time_without_byte_range_promises() {
    let (_directory, services, actor, library, music) = support::library("quality-http").await;
    tones::flac(&music.join("track.flac"), 10);
    support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let source = services
        .catalog
        .sources(&actor, &recording)
        .await
        .unwrap()
        .remove(0)
        .id;
    let session = services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let app = hito_server::router(services);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/sources/{source}/audio?quality=saver&start_ms=5000"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                .header(header::RANGE, "bytes=0-15")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/aac");
    assert_eq!(response.headers()["x-hito-stream-quality"], "saver");
    assert_eq!(response.headers()["x-hito-stream-start-ms"], "5000");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
    assert!(!response.headers().contains_key(header::CONTENT_RANGE));
    assert_ne!(
        response
            .headers()
            .get(header::ACCEPT_RANGES)
            .and_then(|value| value.to_str().ok()),
        Some("bytes")
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let probed = tones::probe(&bytes);
    assert_eq!(probed.codec, "aac");
    assert!((4.0..6.0).contains(&probed.seconds), "{}", probed.seconds);
    let bitrate = bytes.len() as f64 * 8.0 / 5.0 / 1000.0;
    assert!((96.0..160.0).contains(&bitrate), "{bitrate}");
}

#[tokio::test]
async fn original_audio_rejects_a_conversion_time_offset() {
    let library = hito_core::test_support::playback::LocalPlayback::start().await;
    let app = hito_server::router(library.services.clone());
    for query in ["start_ms=1", "quality=original&start_ms=1"] {
        for method in ["GET", "HEAD"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("/api/v1/sources/{}/audio?{query}", library.source))
                        .header(header::AUTHORIZATION, format!("Bearer {}", library.token))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{method} {query}"
            );
        }
    }
}
