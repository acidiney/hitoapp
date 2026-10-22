use audio_fixture::AudioLibrary;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::{AudioRequest, OpenedAudio};
use std::time::Duration;

#[tokio::test]
async fn a_consumer_pause_does_not_expire_ready_audio() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-tail").await;
    let OpenedAudio::Content(mut audio) = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap()
    else {
        panic!("audio must open")
    };
    let mut received = Vec::new();
    while received.len() < 64 {
        received.extend_from_slice(&audio.body.as_mut().unwrap().chunk().await.unwrap().unwrap());
    }
    assert_eq!(received, &f.original[..64]);
    gate.release();
    tokio::time::sleep(Duration::from_secs(16)).await;
    while let Some(bytes) = audio
        .body
        .as_mut()
        .unwrap()
        .chunk()
        .await
        .expect("consumer backpressure must not expire healthy data")
    {
        received.extend_from_slice(&bytes);
    }
    assert_eq!(received, f.original);
}

#[tokio::test]
async fn a_healthy_audio_body_can_continue_beyond_the_control_request_deadline() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-body").await;
    let OpenedAudio::Content(mut audio) = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("ready source must open")
    };
    assert_eq!(audio.length, None);
    let mut body = audio.body.take().unwrap();
    let mut read = 0;
    while read < f.original.len() {
        read += body.chunk().await.unwrap().unwrap().len();
    }
    let waiting = tokio::spawn(async move { body.chunk().await });
    tokio::time::sleep(Duration::from_secs(11)).await;
    gate.release();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn stalled_audio_fails_within_the_read_deadline_and_releases_its_slot() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let _gate = f.server.hold("audio-body").await;
    let OpenedAudio::Content(mut audio) = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap()
    else {
        panic!("ready source must open")
    };
    let mut read = 0;
    while read < f.original.len() {
        read += audio
            .body
            .as_mut()
            .unwrap()
            .chunk()
            .await
            .unwrap()
            .unwrap()
            .len();
    }
    assert!(
        tokio::time::timeout(
            Duration::from_secs(18),
            audio.body.as_mut().unwrap().chunk()
        )
        .await
        .expect("stalled body must have a finite deadline")
        .is_err()
    );
    let first = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    let second = f
        .hito
        .catalog
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap();
    assert!(matches!(first, OpenedAudio::Content(_)));
    assert!(matches!(second, OpenedAudio::Content(_)));
}
