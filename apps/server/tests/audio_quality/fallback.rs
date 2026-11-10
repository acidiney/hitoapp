use crate::{
    fixture::{AudioFixture, no_configuration},
    tones,
};
use axum::http::{StatusCode, header};
use hito_core::test_support::media_tool;
use http_body_util::BodyExt;

#[tokio::test]
async fn low_bitrate_head_reports_original_and_quality_get_requires_an_original_retry() {
    let fixture =
        AudioFixture::start("mp3", |path| tones::mp3(path, 5, 96), no_configuration).await;
    for quality in ["high", "saver"] {
        let query = format!("?quality={quality}&start_ms=1000");
        let head = fixture.response("HEAD", &query).await;
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()["x-hito-stream-quality"], "original");
        assert!(!head.headers().contains_key("x-hito-stream-start-ms"));
        assert_eq!(
            head.headers()[header::CONTENT_LENGTH],
            fixture.original.len().to_string()
        );
        assert!(
            head.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
        let get = fixture.response("GET", &query).await;
        assert_eq!(get.status(), StatusCode::CONFLICT);
        assert_eq!(get.headers()[header::CONTENT_TYPE], "application/json");
        let error: serde_json::Value =
            serde_json::from_slice(&get.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert!(error["error"].as_str().unwrap().contains("original stream"));
    }
    let original = fixture.response("GET", "").await;
    assert_eq!(original.status(), StatusCode::OK);
    assert_eq!(original.headers()["x-hito-stream-quality"], "original");
    assert_eq!(
        original
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        fixture.original
    );
}

#[tokio::test]
async fn converter_startup_failure_never_substitutes_original_audio_at_a_segment_url() {
    let fixture = AudioFixture::start(
        "flac",
        |path| tones::flac(path, 5),
        |config, scratch| {
            config.media_tools.ffmpeg = media_tool::script(
                scratch,
                "ffmpeg",
                "case \"$*\" in *adts*) exit 1;; esac\nexec /usr/bin/ffmpeg \"$@\"",
            );
        },
    )
    .await;
    let query = "?quality=saver&start_ms=2000";
    let head = fixture.response("HEAD", query).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["x-hito-stream-quality"], "saver");
    let get = fixture.response("GET", query).await;
    assert_eq!(get.status(), StatusCode::CONFLICT);
    let error: serde_json::Value =
        serde_json::from_slice(&get.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(error["error"].as_str().unwrap().contains("original stream"));
    let original = fixture.response("GET", "").await;
    assert_eq!(original.status(), StatusCode::OK);
    assert_eq!(
        original
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        fixture.original
    );
}
