use audio_fixture::AudioLibrary;
use axum::http::{HeaderMap, StatusCode};
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::{AudioRequest, ByteRange, OpenedAudio};

#[tokio::test]
async fn provider_audio_uses_only_the_owned_fixed_endpoint_and_yields_original_bytes() {
    let AudioLibrary {
        directory: _dir,
        server,
        hito,
        actor,
        source,
        original,
        ..
    } = AudioLibrary::start().await;
    let headers = HeaderMap::from_iter([(
        "content-type".parse().unwrap(),
        "audio/wav".parse().unwrap(),
    )]);
    server
        .audio(StatusCode::OK, headers, original.clone())
        .await;
    let OpenedAudio::Content(mut audio) = hito
        .catalog
        .open_audio(&actor, &source.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("ready source must open")
    };
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(original.len() as u64));
    assert_eq!(audio.total_length, audio.length);
    assert!(audio.range.is_none());
    let mut bytes = Vec::new();
    let body = audio.body.as_mut().unwrap();
    while let Some(chunk) = body.chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, original);
    let requests = server.requests("v1/audio").await;
    assert_eq!(requests[0].0["authorization"], "Bearer private-audio-token");
    assert!(requests[0].0.get("cookie").is_none());
    assert_eq!(
        requests[0].1["source_id"],
        "audio:/1?private=value&other=bad"
    );
    assert!(requests[0].1.get("other").is_none());

    let headers = HeaderMap::from_iter([
        (
            "content-type".parse().unwrap(),
            "audio/wav".parse().unwrap(),
        ),
        (
            "content-range".parse().unwrap(),
            format!("bytes 44-63/{}", original.len()).parse().unwrap(),
        ),
        ("accept-ranges".parse().unwrap(), "bytes".parse().unwrap()),
    ]);
    server
        .audio(
            StatusCode::PARTIAL_CONTENT,
            headers,
            original[44..64].to_vec(),
        )
        .await;
    let OpenedAudio::Content(mut audio) = hito
        .catalog
        .open_audio(
            &actor,
            &source.id,
            AudioRequest {
                range: Some(ByteRange::From {
                    first: 44,
                    last: Some(63),
                }),
                head: false,
            },
        )
        .await
        .unwrap()
    else {
        panic!("valid byte range must open")
    };
    assert_eq!(audio.range, Some(44..=63));
    assert_eq!(audio.length, Some(20));
    assert_eq!(audio.total_length, Some(original.len() as u64));
    assert!(audio.seekable);
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, original[44..64]);
    assert_eq!(
        server.requests("v1/audio").await[1].0["range"],
        "bytes=44-63"
    );
}
