use crate::{
    fixture::{AudioFixture, no_configuration},
    tones,
};
use axum::http::{StatusCode, header};
use hito_core::test_support::media_tool;
use http_body_util::BodyExt;

#[tokio::test]
async fn head_negotiates_each_quality_without_starting_an_encoder() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("encoder-started");
    let fixture = AudioFixture::start(
        "flac",
        |path| tones::flac(path, 5),
        |config, scratch| {
            config.media_tools.ffmpeg = media_tool::script(
                scratch,
                "ffmpeg",
                &format!(
                    "case \"$*\" in *adts*) touch {};; esac\nexec /usr/bin/ffmpeg \"$@\"",
                    media_tool::quote(&marker)
                ),
            );
        },
    )
    .await;
    for quality in ["high", "saver"] {
        let response = fixture
            .response("HEAD", &format!("?quality={quality}&start_ms=1250"))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/aac");
        assert_eq!(response.headers()["x-hito-stream-quality"], quality);
        assert_eq!(response.headers()["x-hito-stream-start-ms"], "1250");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        assert!(!response.headers().contains_key(header::CONTENT_RANGE));
        assert!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
        assert!(!marker.exists());
    }
}

#[tokio::test]
async fn high_audio_is_aac_near_256_kilobits_and_larger_than_saver() {
    let fixture = AudioFixture::start("flac", |path| tones::flac(path, 10), no_configuration).await;
    let mut bodies = Vec::new();
    for quality in ["high", "saver"] {
        let response = fixture
            .response("GET", &format!("?quality={quality}"))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-hito-stream-quality"], quality);
        assert_eq!(response.headers()["x-hito-stream-start-ms"], "0");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/aac");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let probed = tones::probe(&bytes);
        assert_eq!(probed.codec, "aac");
        assert!((9.0..11.0).contains(&probed.seconds), "{}", probed.seconds);
        bodies.push(bytes);
    }
    let high_rate = bodies[0].len() as f64 * 8.0 / 10.0 / 1000.0;
    assert!((192.0..320.0).contains(&high_rate), "{high_rate}");
    assert!(bodies[0].len() > bodies[1].len() * 3 / 2);
}

#[tokio::test]
async fn default_and_explicit_original_preserve_ranges_and_if_range_restarts() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let fixture = AudioFixture::start(
        "wav",
        |path| {
            hito_core::test_support::support::write_wav(path, 0);
        },
        no_configuration,
    )
    .await;
    for query in ["", "?quality=original", "?quality=original&start_ms=0"] {
        for if_range in [false, true] {
            let mut request = Request::builder()
                .uri(format!("/api/v1/sources/{}/audio{query}", fixture.source))
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.token))
                .header(header::RANGE, "bytes=44-63");
            if if_range {
                request = request.header(header::IF_RANGE, "\"stale-version\"");
            }
            let response = fixture
                .app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if if_range {
                    StatusCode::OK
                } else {
                    StatusCode::PARTIAL_CONTENT
                }
            );
            assert_eq!(response.headers()["x-hito-stream-quality"], "original");
            assert!(!response.headers().contains_key("x-hito-stream-start-ms"));
            assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                bytes.as_ref(),
                if if_range {
                    fixture.original.as_slice()
                } else {
                    &fixture.original[44..64]
                }
            );
        }
    }
}
