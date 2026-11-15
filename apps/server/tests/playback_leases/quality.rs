use super::fixture::Case;
use axum::http::{StatusCode, header};
use hito_core::{PlaybackLease, test_support::hang_guard::hang_guard};
use hito_testkit::tone_files as tones;
use http_body_util::BodyExt;

async fn flac_case() -> Case {
    Case::with_named_audio("private-owned.flac", |path| {
        tones::flac(path, 10);
        std::fs::read(path).unwrap()
    })
    .await
}

async fn audio_path(f: &Case) -> Option<String> {
    let response = f.begin().await;
    if response.status() != StatusCode::OK {
        return None;
    }
    let lease: PlaybackLease =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).ok()?;
    Some(format!("/api/v1/playback/leases/{}/audio", lease.id))
}

struct Converted {
    status: StatusCode,
    content_type: Option<String>,
    quality: Option<String>,
    start_ms: Option<String>,
    cache_control: Option<String>,
    content_length: bool,
    content_range: bool,
    codec: String,
    seconds: f64,
}

#[tokio::test]
async fn a_saver_lease_stream_is_aac_from_the_requested_time_without_byte_ranges() {
    let f = hang_guard("the lease quality fixture must start", flac_case()).await;
    let path = audio_path(&f).await.expect("the lease must begin");
    let response = f
        .request(
            "GET",
            &format!("{path}?quality=saver&start_ms=5000"),
            &[("range", "bytes=0-15")],
            None,
        )
        .await;
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value: &header::HeaderValue| value.to_str().ok())
            .map(str::to_owned)
    };
    let mut converted = Converted {
        status: response.status(),
        content_type: header(header::CONTENT_TYPE),
        quality: header(header::HeaderName::from_static("x-hito-stream-quality")),
        start_ms: header(header::HeaderName::from_static("x-hito-stream-start-ms")),
        cache_control: header(header::CACHE_CONTROL),
        content_length: response.headers().contains_key(header::CONTENT_LENGTH),
        content_range: response.headers().contains_key(header::CONTENT_RANGE),
        codec: String::new(),
        seconds: 0.0,
    };
    let bytes = hang_guard(
        "the converted lease body must finish",
        response.into_body().collect(),
    )
    .await
    .unwrap()
    .to_bytes();
    if converted.content_type.as_deref() == Some("audio/aac") {
        let probed = tones::probe(&bytes);
        converted.codec = probed.codec;
        converted.seconds = probed.seconds;
    }
    f.close().await;

    assert_eq!(converted.status, StatusCode::OK);
    assert_eq!(converted.content_type.as_deref(), Some("audio/aac"));
    assert_eq!(converted.quality.as_deref(), Some("saver"));
    assert_eq!(converted.start_ms.as_deref(), Some("5000"));
    assert_eq!(
        converted.cache_control.as_deref(),
        Some("private, no-store")
    );
    assert!(!converted.content_length && !converted.content_range);
    assert_eq!(converted.codec, "aac");
    assert!(
        (4.0..6.0).contains(&converted.seconds),
        "{}",
        converted.seconds
    );
}

async fn mp3_case() -> Case {
    Case::with_named_audio("private-owned.mp3", |path| {
        tones::mp3(path, 5, 96);
        std::fs::read(path).unwrap()
    })
    .await
}

fn header_of(response: &axum::response::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[tokio::test]
async fn a_converted_lease_head_reports_the_quality_without_a_body() {
    let f = hang_guard("the lease quality fixture must start", flac_case()).await;
    let path = audio_path(&f).await.expect("the lease must begin");
    let head = f
        .request(
            "HEAD",
            &format!("{path}?quality=high&start_ms=2000"),
            &[],
            None,
        )
        .await;
    let status = head.status();
    let quality = header_of(&head, "x-hito-stream-quality");
    let start = header_of(&head, "x-hito-stream-start-ms");
    let content_type = header_of(&head, "content-type");
    let length = head.headers().contains_key(header::CONTENT_LENGTH);
    let body = head.into_body().collect().await.unwrap().to_bytes();
    f.close().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(quality.as_deref(), Some("high"));
    assert_eq!(start.as_deref(), Some("2000"));
    assert_eq!(content_type.as_deref(), Some("audio/aac"));
    assert!(!length);
    assert!(body.is_empty());
}

#[tokio::test]
async fn a_low_bitrate_lease_heads_original_and_asks_get_to_retry_the_original() {
    let f = hang_guard("the lease mp3 fixture must start", mp3_case()).await;
    let path = audio_path(&f).await.expect("the lease must begin");
    let head = f
        .request(
            "HEAD",
            &format!("{path}?quality=saver&start_ms=1000"),
            &[],
            None,
        )
        .await;
    let head_facts = (
        head.status(),
        header_of(&head, "x-hito-stream-quality"),
        header_of(&head, "x-hito-stream-start-ms"),
        header_of(&head, "content-length"),
    );
    let get = f
        .request(
            "GET",
            &format!("{path}?quality=saver&start_ms=1000"),
            &[],
            None,
        )
        .await;
    let get_status = get.status();
    let original = f.request("GET", &path, &[], None).await;
    let original_status = original.status();
    let original_bytes = original.into_body().collect().await.unwrap().to_bytes();
    let expected = f.original.clone();
    f.close().await;
    assert_eq!(head_facts.0, StatusCode::OK);
    assert_eq!(head_facts.1.as_deref(), Some("original"));
    assert_eq!(head_facts.2, None);
    assert_eq!(head_facts.3, Some(expected.len().to_string()));
    assert_eq!(get_status, StatusCode::CONFLICT);
    assert_eq!(original_status, StatusCode::OK);
    assert_eq!(original_bytes.as_ref(), expected.as_slice());
}

#[tokio::test]
async fn lease_quality_queries_are_validated() {
    let f = hang_guard("the lease quality fixture must start", flac_case()).await;
    let path = audio_path(&f).await.expect("the lease must begin");
    let mut statuses = Vec::new();
    for query in [
        "?start_ms=1",
        "?quality=original&start_ms=1",
        "?quality=ultra",
        "?quality=saver&start_ms=604800001",
        "?quality=saver&extra=1",
    ] {
        for method in ["GET", "HEAD"] {
            statuses.push(
                f.request(method, &format!("{path}{query}"), &[], None)
                    .await
                    .status(),
            );
        }
    }
    f.close().await;
    assert_eq!(
        statuses,
        [
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::BAD_REQUEST,
            StatusCode::BAD_REQUEST,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::BAD_REQUEST,
            StatusCode::BAD_REQUEST,
        ]
    );
}

#[tokio::test]
async fn two_concurrent_lease_conversions_each_decode_and_a_third_body_is_refused() {
    let f = hang_guard("the lease quality fixture must start", flac_case()).await;
    let path = audio_path(&f).await.expect("the lease must begin");
    let first = f
        .request(
            "GET",
            &format!("{path}?quality=saver&start_ms=1000"),
            &[],
            None,
        )
        .await;
    let second = f
        .request(
            "GET",
            &format!("{path}?quality=high&start_ms=6000"),
            &[],
            None,
        )
        .await;
    let third = f
        .request("GET", &format!("{path}?quality=saver"), &[], None)
        .await;
    let statuses = (first.status(), second.status(), third.status());
    drop(third);
    let (first, second) = hang_guard("both converted lease bodies must finish", async {
        tokio::join!(first.into_body().collect(), second.into_body().collect())
    })
    .await;
    let first = tones::probe(&first.unwrap().to_bytes());
    let second = tones::probe(&second.unwrap().to_bytes());
    f.close().await;
    assert_eq!(
        statuses,
        (StatusCode::OK, StatusCode::OK, StatusCode::CONFLICT)
    );
    assert_eq!(
        (first.codec.as_str(), second.codec.as_str()),
        ("aac", "aac")
    );
    assert!((8.0..10.0).contains(&first.seconds), "{}", first.seconds);
    assert!((3.0..5.0).contains(&second.seconds), "{}", second.seconds);
}

#[tokio::test]
async fn releasing_a_lease_stops_its_conversion_and_frees_the_slot() {
    let f = hang_guard("the lease quality fixture must start", flac_case()).await;
    let mut released = Vec::new();
    for _ in 0..3 {
        let path = audio_path(&f).await.expect("the lease must begin");
        let response = f
            .request("GET", &format!("{path}?quality=saver"), &[], None)
            .await;
        let status = response.status();
        let mut body = response.into_body();
        let first = body.frame().await.map(|frame| frame.is_ok());
        let release = f
            .request("DELETE", path.trim_end_matches("/audio"), &[], None)
            .await
            .status();
        let rest = hang_guard("a released conversion must stop", body.collect()).await;
        released.push((status, first, release, rest.is_err()));
    }
    let path = audio_path(&f).await.expect("the lease must begin");
    let after = f
        .request(
            "GET",
            &format!("{path}?quality=saver&start_ms=8000"),
            &[],
            None,
        )
        .await;
    let after_status = after.status();
    let after_bytes = after.into_body().collect().await.unwrap().to_bytes();
    f.close().await;
    for (status, first, release, stopped) in released {
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first, Some(true));
        assert_eq!(release, StatusCode::NO_CONTENT);
        assert!(
            stopped,
            "a released lease must not finish its converted body"
        );
    }
    assert_eq!(after_status, StatusCode::OK);
    assert_eq!(tones::probe(&after_bytes).codec, "aac");
}
