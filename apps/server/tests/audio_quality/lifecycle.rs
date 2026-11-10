use crate::{
    fixture::{AudioFixture, no_configuration},
    tones,
};
use axum::http::StatusCode;
use http_body_util::BodyExt;

#[tokio::test]
async fn logout_stops_prefetched_http_audio_and_rejects_new_quality_requests() {
    let fixture = AudioFixture::start("flac", |path| tones::flac(path, 5), no_configuration).await;
    let response = fixture.response("GET", "?quality=saver").await;
    assert_eq!(response.status(), StatusCode::OK);
    fixture
        .services
        .identity
        .logout(&fixture.token)
        .await
        .unwrap();
    let error = response.into_body().collect().await.unwrap_err();
    assert!(error.to_string().contains("Audio stream could not finish"));
    for method in ["GET", "HEAD"] {
        assert_eq!(
            fixture.response(method, "?quality=saver").await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn dropping_an_http_body_joins_its_converter_and_releases_admission() {
    use hito_core::test_support::media_tool;
    use std::{path::Path, time::Duration};
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("converter-pid");
    let fixture = AudioFixture::start(
        "flac",
        |path| tones::flac(path, 60),
        |config, scratch| {
            config.conversion_parallelism = 1;
            config.media_tools.ffmpeg = media_tool::script(
                scratch,
                "ffmpeg",
                &format!(
                    "case \"$*\" in *adts*) echo $$ > {};; esac\nexec /usr/bin/ffmpeg \"$@\"",
                    media_tool::quote(&marker)
                ),
            );
        },
    )
    .await;
    let response = fixture.response("GET", "?quality=saver").await;
    assert_eq!(response.status(), StatusCode::OK);
    let pid = std::fs::read_to_string(&marker).unwrap();
    let process = format!("/proc/{}", pid.trim());
    assert!(Path::new(&process).exists());
    assert_eq!(
        fixture.response("GET", "?quality=saver").await.status(),
        StatusCode::CONFLICT
    );
    drop(response);
    tokio::time::timeout(Duration::from_secs(3), async {
        while Path::new(&process).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("dropping the body must join the converter");
    assert_eq!(
        fixture.response("GET", "?quality=saver").await.status(),
        StatusCode::OK
    );
}
