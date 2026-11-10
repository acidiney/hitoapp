use super::fixture::*;
use std::time::Duration;

#[tokio::test]
async fn authorized_opus_decodes_to_bounded_pcm_at_the_original_rate() {
    let library = library(2, no_configuration).await;
    let mut stream = library
        .grant()
        .await
        .open_pcm(Duration::ZERO)
        .await
        .unwrap();
    let format = stream.format();
    assert_eq!(format.sample_rate, 48_000);
    assert_eq!(format.channels, 1);
    assert!((1.99..2.02).contains(&format.duration.unwrap().as_secs_f64()));
    let mut samples = Vec::new();
    while let Some(block) = stream.next_block().await.unwrap() {
        assert!(block.samples.len() <= 8_192);
        assert!(block.samples.iter().all(|value| value.is_finite()));
        assert_eq!(
            block.position,
            Duration::from_secs_f64(samples.len() as f64 / 48_000.)
        );
        samples.extend(block.samples);
    }
    assert_eq!(samples.len(), 96_000);
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0. && pair[1] > 0.)
        .count();
    assert!((879..=881).contains(&crossings), "{crossings}");
    stream.close().await.unwrap();
}

#[tokio::test]
async fn unsupported_profiles_have_a_typed_failure_distinct_from_failed_decoding() {
    let library = library(2, |config, tools| {
        config.media_tools.ffprobe = hito_core::test_support::media_tool::script(tools, "ffprobe", "case \"$*\" in *stream=codec_name,sample_rate,channels,duration*) printf '%s' '{\"streams\":[{\"codec_name\":\"unrecognized\",\"sample_rate\":\"48000\",\"channels\":1}]}'; exit 0;; esac\nexec /usr/bin/ffprobe \"$@\"");
    }).await;
    let result = library.grant().await.open_pcm(Duration::ZERO).await;
    assert!(matches!(result, Err(hito_core::Error::UnsupportedAudio)));
}

#[tokio::test]
async fn reopening_at_zero_backward_and_submillisecond_offsets_preserves_source_time() {
    let library = library(6, no_configuration).await;
    let grant = library.grant().await;
    let mut whole = grant.open_pcm(Duration::ZERO).await.unwrap();
    let format = whole.format();
    let mut expected = Vec::new();
    while let Some(block) = whole.next_block().await.unwrap() {
        expected.extend(block.samples);
    }
    whole.close().await.unwrap();
    for first_frame in [192_000, 48_000, 24, 1, 0] {
        let start = Duration::from_secs_f64(first_frame as f64 / 48_000.);
        let mut tail = grant.open_pcm(start).await.unwrap();
        assert_eq!(tail.format(), format);
        let mut actual = Vec::new();
        while let Some(block) = tail.next_block().await.unwrap() {
            assert_eq!(
                block.position,
                Duration::from_secs_f64((first_frame + actual.len()) as f64 / 48_000.)
            );
            actual.extend(block.samples);
        }
        assert_eq!(actual.len(), expected.len() - first_frame);
        let error = actual
            .iter()
            .zip(&expected[first_frame..])
            .map(|(left, right)| (left - right).powi(2))
            .sum::<f32>()
            / actual.len() as f32;
        assert!(
            error.sqrt() < 0.003,
            "frame={first_frame} full_rms={}",
            error.sqrt()
        );
        tail.close().await.unwrap();
    }
}

#[tokio::test]
async fn decoding_preserves_the_original_and_never_creates_download_intent() {
    let library = library(2, no_configuration).await;
    let original = library
        .directory
        .path()
        .join("music/private-recording.opus");
    let before = std::fs::read(&original).unwrap();
    let recording = library
        .services
        .catalog
        .recordings(&library.actor, hito_core::Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    assert!(matches!(
        library
            .services
            .downloads
            .status(&library.actor, &recording)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    let mut stream = library
        .grant()
        .await
        .open_pcm(Duration::ZERO)
        .await
        .unwrap();
    while stream.next_block().await.unwrap().is_some() {}
    stream.close().await.unwrap();
    assert_eq!(std::fs::read(original).unwrap(), before);
    assert!(matches!(
        library
            .services
            .downloads
            .status(&library.actor, &recording)
            .await,
        Err(hito_core::Error::NotFound)
    ));
}

#[tokio::test]
async fn opening_at_the_known_end_returns_stable_eof_instead_of_a_decoder_failure() {
    let library = library(2, no_configuration).await;
    let grant = library.grant().await;
    let stream = grant.open_pcm(Duration::ZERO).await.unwrap();
    let format = stream.format();
    stream.close().await.unwrap();
    let mut end = grant.open_pcm(format.duration.unwrap()).await.unwrap();
    assert_eq!(end.format(), format);
    assert!(end.next_block().await.unwrap().is_none());
    assert!(end.next_block().await.unwrap().is_none());
    end.close().await.unwrap();
}
