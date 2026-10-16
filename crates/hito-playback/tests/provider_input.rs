#[path = "../../hito-core/tests/fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;
use std::{io::Read, time::Duration};

#[tokio::test]
async fn provider_audio_is_decoded_through_an_authorized_bounded_input() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let grant = f
        .hito
        .catalog
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let source = hito_playback::Source::open(grant).await.unwrap();
    tokio::task::spawn_blocking(move || {
        let mut decoder = hito_audio::Decoder::open(source).unwrap();
        let mut samples = 0;
        while let Some(block) = decoder.read().unwrap() {
            samples += block.samples.len();
        }
        assert_eq!(samples, 8000);
    })
    .await
    .unwrap();
    let requests = f.server.requests("v1/audio").await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0["authorization"], "Bearer private-audio-token");
}

#[tokio::test]
async fn an_owner_can_interrupt_a_pending_provider_read_without_waiting_for_data() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-tail").await;
    let grant = f
        .hito
        .catalog
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let mut source = hito_playback::Source::open(grant).await.unwrap();
    let stop = source.stop_handle();
    let mut reading = tokio::task::spawn_blocking(move || {
        source.read_exact(&mut [0; 64]).unwrap();
        source.read_exact(&mut [0; 8]).unwrap_err().kind()
    });
    gate.entered().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut reading)
            .await
            .is_err()
    );
    stop.request();
    let result = tokio::time::timeout(Duration::from_secs(2), reading).await;
    gate.release();
    assert_eq!(
        result.expect("stop must interrupt a held input").unwrap(),
        std::io::ErrorKind::ConnectionAborted
    );
}

#[tokio::test]
async fn short_valid_ranges_continue_until_the_representation_ends() {
    use std::io::{Seek, SeekFrom};
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    f.server.range_chunks(1024).await;
    let grant = f
        .hito
        .catalog
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let mut source = hito_playback::Source::open(grant).await.unwrap();
    let received = tokio::task::spawn_blocking(move || {
        source.seek(SeekFrom::Start(8192)).unwrap();
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).unwrap();
        bytes
    })
    .await
    .unwrap();
    assert!(
        received == f.original[8192..],
        "range continuation returned {} of {} bytes",
        received.len(),
        f.original.len() - 8192
    );
}

#[tokio::test]
async fn a_failed_range_request_cannot_turn_into_clean_end_of_input() {
    use std::io::{Seek, SeekFrom};
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    f.server.range_chunks(1024).await;
    let grant = f
        .hito
        .catalog
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let mut source = hito_playback::Source::open(grant).await.unwrap();
    f.server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
    tokio::task::spawn_blocking(move || {
        assert!(source.seek(SeekFrom::Start(100)).is_err());
        assert!(
            source.read(&mut [0; 1]).is_err(),
            "a failed input must not report successful EOF"
        );
    })
    .await
    .unwrap();
}
