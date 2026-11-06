use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::{AudioRequest, ByteRange, Error, OpenedAudio, StreamPace};
use std::time::{Duration, Instant};

async fn open(f: &AudioLibrary) -> hito_core::Result<OpenedAudio> {
    f.hito
        .playback
        .open_audio(&f.actor, &f.source.id, AudioRequest::default())
        .await
}

async fn drain(opened: OpenedAudio) -> hito_core::Result<Vec<u8>> {
    let OpenedAudio::Content(mut audio) = opened else {
        panic!("audio must open")
    };
    let mut body = audio.body.take().unwrap();
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await? {
        received.extend_from_slice(&chunk);
    }
    Ok(received)
}

#[tokio::test]
async fn expired_links_and_server_errors_fail_each_open_once_and_recover_when_the_provider_does() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let failures = [
        StatusCode::FORBIDDEN,
        StatusCode::GONE,
        StatusCode::NOT_FOUND,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::GATEWAY_TIMEOUT,
    ];
    f.server.audio_statuses(failures.to_vec());
    for status in failures {
        let started = Instant::now();
        let error = match open(&f).await {
            Ok(_) => panic!("{status} must not open audio"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Upstream(_)), "{status}: {error:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "{status}");
    }
    assert_eq!(
        f.server.requests("v1/audio").await.len(),
        failures.len(),
        "the relay must not retry a failed open by itself"
    );
    assert_eq!(
        f.hito
            .catalog
            .sources(&f.actor, &f.source.recording_id)
            .await
            .unwrap()
            .len(),
        1,
        "a failing provider link must not remove the saved source"
    );
    assert_eq!(drain(open(&f).await.unwrap()).await.unwrap(), f.original);
    assert_eq!(
        f.server.requests("v1/audio").await.len(),
        failures.len() + 1
    );
}

#[tokio::test]
async fn a_slow_first_byte_is_waited_for_within_its_bound_and_a_silent_provider_fails_at_it() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server.audio_first_byte_delay(Duration::from_secs(2));
    let started = Instant::now();
    assert_eq!(drain(open(&f).await.unwrap()).await.unwrap(), f.original);
    assert!(started.elapsed() >= Duration::from_secs(2));

    f.server.audio_first_byte_delay(Duration::ZERO);
    let gate = f.server.hold("v1/audio").await;
    let started = Instant::now();
    let error = match open(&f).await {
        Ok(_) => panic!("a silent provider must not open audio"),
        Err(error) => error,
    };
    let waited = started.elapsed();
    assert!(matches!(error, Error::Upstream(_)), "{error:?}");
    assert!(
        (Duration::from_secs(9)..Duration::from_secs(13)).contains(&waited),
        "the header wait must end at its ten second bound, not {waited:?}"
    );
    gate.open();
    let first = open(&f).await.unwrap();
    let second = open(&f).await.unwrap();
    assert!(matches!(first, OpenedAudio::Content(_)));
    assert!(matches!(second, OpenedAudio::Content(_)));
}

#[tokio::test]
async fn a_slow_trickling_body_completes_intact_without_a_total_time_limit() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let chunk = f.original.len() / 6 + 1;
    f.server.audio_trickle(chunk, Duration::from_millis(900));
    let started = Instant::now();
    let received = drain(open(&f).await.unwrap()).await.unwrap();
    assert_eq!(received, f.original);
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "the fixture must really trickle"
    );
}

#[tokio::test]
async fn a_connection_reset_mid_body_ends_the_stream_with_an_error_and_releases_its_slot() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold_broken_tail(64).await;
    let OpenedAudio::Content(mut audio) = open(&f).await.unwrap() else {
        panic!("audio headers must open")
    };
    let mut body = audio.body.take().unwrap();
    let mut received = Vec::new();
    while received.len() < 64 {
        received.extend_from_slice(&body.chunk().await.unwrap().unwrap());
    }
    assert_eq!(received, &f.original[..64]);
    gate.open();
    let outcome = tokio::time::timeout(Duration::from_secs(5), body.chunk())
        .await
        .expect("a reset must end the stream promptly");
    assert!(matches!(outcome, Err(Error::Upstream(_))), "{outcome:?}");
    drop(body);
    f.server.clear_audio_tail_fault().await;
    let first = open(&f).await.unwrap();
    let second = open(&f).await.unwrap();
    assert!(matches!(first, OpenedAudio::Content(_)));
    assert!(matches!(second, OpenedAudio::Content(_)));
}

#[tokio::test]
async fn a_provider_that_ignores_a_seek_range_is_reported_as_a_whole_body_not_a_partial_one() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server.audio_ignores_ranges();
    let OpenedAudio::Content(mut audio) = f
        .hito
        .playback
        .open_audio(
            &f.actor,
            &f.source.id,
            AudioRequest {
                range: Some(ByteRange::From {
                    first: 44,
                    last: None,
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    else {
        panic!("an ignored range still opens the whole audio")
    };
    assert!(audio.range.is_none());
    assert_eq!(audio.length, Some(f.original.len() as u64));
    let mut body = audio.body.take().unwrap();
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, f.original);
    assert_eq!(
        f.server.requests("v1/audio").await[0].0["range"],
        "bytes=44-"
    );
}

fn pace(floor_bytes_per_second: u64) -> StreamPace {
    StreamPace {
        window: Duration::from_millis(1800),
        floor_bytes_per_second,
        ..StreamPace::DEFAULT
    }
}

#[tokio::test]
async fn a_body_below_the_throughput_floor_is_cut_off_within_the_window_and_frees_its_slot() {
    let f = AudioLibrary::start_paced(pace(6000)).await;
    f.ready().await;
    f.server
        .audio_trickle(f.original.len() / 6 + 1, Duration::from_millis(900));
    let OpenedAudio::Content(mut audio) = open(&f).await.unwrap() else {
        panic!("audio headers must open")
    };
    let mut body = audio.body.take().unwrap();
    let started = Instant::now();
    let mut received = 0;
    let error = loop {
        match body.chunk().await {
            Ok(Some(chunk)) => received += chunk.len(),
            Ok(None) => panic!("a stream below the floor must not complete"),
            Err(error) => break error,
        }
    };
    assert!(matches!(error, Error::StreamTooSlow), "{error:?}");
    assert!(
        started.elapsed() < Duration::from_millis(3600),
        "{:?}",
        started.elapsed()
    );
    assert!(received < f.original.len());
    f.server.audio_trickle(f.original.len(), Duration::ZERO);
    let first = open(&f).await.unwrap();
    let second = open(&f).await.unwrap();
    assert!(matches!(first, OpenedAudio::Content(_)));
    assert!(matches!(second, OpenedAudio::Content(_)));
    drop(body);
}

#[tokio::test]
async fn a_slow_body_above_the_throughput_floor_completes_intact() {
    let f = AudioLibrary::start_paced(pace(1000)).await;
    f.ready().await;
    f.server
        .audio_trickle(f.original.len() / 6 + 1, Duration::from_millis(900));
    let started = Instant::now();
    let received = drain(open(&f).await.unwrap()).await.unwrap();
    assert_eq!(received, f.original);
    assert!(started.elapsed() >= Duration::from_secs(4));
}
