use axum::http::StatusCode;
use hito_core::test_support::provider_audio::{self as audio_fixture, AudioLibrary};
use std::{
    io::{Read, Seek, SeekFrom},
    time::{Duration, Instant},
};

async fn grant(f: &AudioLibrary) -> hito_core::PlaybackSource {
    f.hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_connection_reset_mid_body_fails_the_input_for_good_instead_of_ending_cleanly() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold_broken_tail(64).await;
    let mut source = hito_playback::Source::open(grant(&f).await).await.unwrap();
    let released = gate.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        source.read_exact(&mut [0; 64]).unwrap();
        released.open();
        let started = Instant::now();
        let first = source.read(&mut [0; 8]);
        let again = source.read(&mut [0; 8]);
        (first.is_err(), again.is_err(), started.elapsed())
    });
    let (first, again, elapsed) = tokio::time::timeout(Duration::from_secs(10), outcome)
        .await
        .expect("a reset must not hang the audio thread")
        .unwrap();
    assert!(first, "the reset must surface as a read error");
    assert!(
        again,
        "a failed input must stay failed, never report a clean end"
    );
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

#[tokio::test]
async fn a_body_shorter_than_its_announced_length_is_an_error_not_the_end_of_the_track() {
    let f = AudioLibrary::start().await;
    let mut headers = audio_fixture::headers();
    headers.insert(
        "content-length",
        (f.original.len() + 4).to_string().parse().unwrap(),
    );
    f.server
        .audio(StatusCode::OK, headers, f.original.clone())
        .await;
    let gate = f.server.hold("audio-body").await;
    let mut source = hito_playback::Source::open(grant(&f).await).await.unwrap();
    let length = f.original.len();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut received = vec![0; length];
        source.read_exact(&mut received).unwrap();
        gate.release();
        let mut tail = [0; 16];
        (received, source.read(&mut tail).is_err())
    });
    let (received, failed) = tokio::time::timeout(Duration::from_secs(10), outcome)
        .await
        .expect("a short body must not hang the audio thread")
        .unwrap();
    assert_eq!(received, f.original);
    assert!(failed);
}

#[tokio::test]
async fn an_expired_link_on_a_seek_fails_once_without_retrying_and_leaves_the_input_failed() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server.range_chunks(1024).await;
    let mut source = hito_playback::Source::open(grant(&f).await).await.unwrap();
    f.server
        .audio_statuses(vec![StatusCode::GONE, StatusCode::FORBIDDEN]);
    let started = Instant::now();
    let outcome = tokio::task::spawn_blocking(move || {
        let seek = source.seek(SeekFrom::Start(8192));
        let read = source.read(&mut [0; 1]);
        (seek.is_err(), read.is_err())
    })
    .await
    .unwrap();
    assert_eq!(outcome, (true, true));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        f.server.requests("v1/audio").await.len(),
        2,
        "one open and exactly one failed seek request"
    );
}

#[tokio::test]
async fn a_provider_that_ignores_the_seek_range_is_never_decoded_from_the_wrong_offset() {
    let f = AudioLibrary::start().await;
    let mut headers = audio_fixture::headers();
    headers.insert("accept-ranges", "bytes".parse().unwrap());
    f.server
        .audio(StatusCode::OK, headers, f.original.clone())
        .await;
    f.server.audio_ignores_ranges();
    let mut source = hito_playback::Source::open(grant(&f).await).await.unwrap();
    let outcome = tokio::task::spawn_blocking(move || {
        let seek = source.seek(SeekFrom::Start(8192));
        let read = source.read(&mut [0; 1]);
        (seek.is_err(), read.is_err())
    })
    .await
    .unwrap();
    assert_eq!(outcome, (true, true));
}

#[tokio::test]
async fn an_input_that_never_promised_ranges_refuses_a_seek_and_keeps_playing_from_where_it_is() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let mut source = hito_playback::Source::open(grant(&f).await).await.unwrap();
    let outcome = tokio::task::spawn_blocking(move || {
        source.read_exact(&mut [0; 100]).unwrap();
        let seek = source.seek(SeekFrom::Start(8192)).unwrap_err().kind();
        let mut rest = Vec::new();
        source.read_to_end(&mut rest).unwrap();
        (seek, rest.len())
    })
    .await
    .unwrap();
    assert_eq!(
        outcome,
        (std::io::ErrorKind::Unsupported, f.original.len() - 100)
    );
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);
}

#[tokio::test]
async fn a_slow_trickling_provider_still_decodes_every_sample() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server
        .audio_trickle(f.original.len() / 6 + 1, Duration::from_millis(900));
    let source = hito_playback::Source::open(grant(&f).await).await.unwrap();
    let started = Instant::now();
    let samples = tokio::task::spawn_blocking(move || {
        let mut decoder = hito_audio::Decoder::open(source).unwrap();
        let mut samples = 0;
        while let Some(block) = decoder.read().unwrap() {
            samples += block.samples.len();
        }
        samples
    })
    .await
    .unwrap();
    assert_eq!(samples, 8000);
    assert!(started.elapsed() >= Duration::from_secs(4));
}

#[tokio::test]
async fn a_slow_first_byte_delays_the_open_and_a_silent_provider_fails_it_at_the_bound() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server.audio_first_byte_delay(Duration::from_secs(2));
    let started = Instant::now();
    hito_playback::Source::open(grant(&f).await).await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(2));

    f.server.audio_first_byte_delay(Duration::ZERO);
    let gate = f.server.hold("v1/audio").await;
    let started = Instant::now();
    assert!(hito_playback::Source::open(grant(&f).await).await.is_err());
    let waited = started.elapsed();
    assert!(
        (Duration::from_secs(9)..Duration::from_secs(13)).contains(&waited),
        "{waited:?}"
    );
    gate.open();
    hito_playback::Source::open(grant(&f).await).await.unwrap();
}
