use hito_core::test_support::playback as fixture;
use std::io::{Read, Seek, SeekFrom};

#[tokio::test]
async fn authorized_local_input_streams_and_seeks_without_exposing_a_path_to_the_decoder() {
    let f = fixture::LocalPlayback::start().await;
    let grant = f
        .services
        .catalog
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    let mut source = hito_playback::Source::open(grant).await.unwrap();
    let expected = f.original;
    tokio::task::spawn_blocking(move || {
        let mut head = [0; 44];
        source.read_exact(&mut head).unwrap();
        assert_eq!(head, expected[..44]);
        assert_eq!(source.seek(SeekFrom::Start(512)).unwrap(), 512);
        let mut bytes = [0; 128];
        source.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, expected[512..640]);
        source.seek(SeekFrom::Start(0)).unwrap();
        let mut decoder = hito_audio::Decoder::open(source).unwrap();
        assert_eq!(decoder.format().sample_rate, 8000);
        let mut count = 0;
        while let Some(block) = decoder.read().unwrap() {
            count += block.samples.len();
        }
        assert_eq!(count, 8000);
    })
    .await
    .unwrap();
}
