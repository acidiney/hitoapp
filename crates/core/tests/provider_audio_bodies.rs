use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use hito_core::OpenedAudio;
use hito_core::test_support::provider_audio as audio_fixture;

#[tokio::test]
async fn provider_chunks_yield_at_most_64_kib_without_losing_audio() {
    let f = AudioLibrary::start().await;
    let bytes = f.original.repeat(128);
    f.server
        .audio(StatusCode::OK, audio_fixture::headers(), bytes.clone())
        .await;
    let OpenedAudio::Content(mut audio) = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap()
    else {
        panic!("audio must open")
    };
    let mut received = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        assert!(
            chunk.len() <= 65536,
            "audio must yield at most one 64KiB chunk"
        );
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, bytes);
}

#[tokio::test]
async fn premature_audio_eof_is_an_error_without_deleting_the_saved_source() {
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
    let OpenedAudio::Content(mut audio) = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap()
    else {
        panic!("audio headers must open")
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
    gate.release();
    assert!(audio.body.as_mut().unwrap().chunk().await.is_err());
    assert_eq!(
        f.hito
            .catalog
            .sources(&f.actor, &f.source.recording_id)
            .await
            .unwrap()[0]
            .id,
        f.source.id
    );
}
