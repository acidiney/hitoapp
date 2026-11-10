use crate::conversion_support::*;
use hito_core::test_support::media_tool;
use hito_core::{Error, OpenedAudio, QualityAudio, StreamQuality};

fn kilobits_per_second(bytes: &[u8], seconds: f64) -> f64 {
    bytes.len() as f64 * 8.0 / seconds / 1000.0
}

#[tokio::test]
async fn a_lossless_source_converts_to_aac_in_the_data_saver_bitrate_class() {
    let library = library(10, "flac", flac, no_configuration).await;
    let audio = open(&library, request(StreamQuality::DataSaver, 0))
        .await
        .unwrap();
    let QualityAudio::Converted(converted) = &audio else {
        panic!("a lossless source must be converted");
    };
    assert_eq!(converted.content_type, "audio/aac");
    assert_eq!(converted.bitrate, 128_000);
    let bytes = converted_bytes(audio).await;
    let probed = tones::probe(&bytes);
    assert_eq!(probed.codec, "aac");
    assert!((9.0..11.0).contains(&probed.seconds), "{}", probed.seconds);
    let rate = kilobits_per_second(&bytes, 10.0);
    assert!((96.0..160.0).contains(&rate), "{rate}");
}

#[tokio::test]
async fn the_high_class_is_larger_than_data_saver_and_near_its_target() {
    let library = library(10, "flac", flac, no_configuration).await;
    let high = converted_bytes(
        open(&library, request(StreamQuality::High, 0))
            .await
            .unwrap(),
    )
    .await;
    let saver = converted_bytes(
        open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .unwrap(),
    )
    .await;
    let rate = kilobits_per_second(&high, 10.0);
    assert!((192.0..320.0).contains(&rate), "{rate}");
    assert!(high.len() > saver.len() * 3 / 2);
}

#[tokio::test]
async fn a_start_offset_skips_the_audio_before_it() {
    let library = library(10, "flac", flac, no_configuration).await;
    let whole = converted_bytes(
        open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .unwrap(),
    )
    .await;
    let tail = converted_bytes(
        open(&library, request(StreamQuality::DataSaver, 5))
            .await
            .unwrap(),
    )
    .await;
    let ratio = tail.len() as f64 / whole.len() as f64;
    assert!((0.4..0.6).contains(&ratio), "{ratio}");
    let probed = tones::probe(&tail);
    assert!((4.0..6.0).contains(&probed.seconds), "{}", probed.seconds);
}

#[tokio::test]
async fn a_lossy_source_at_or_below_the_target_is_streamed_unchanged() {
    let library = library(
        5,
        "mp3",
        |path, seconds| tones::mp3(path, seconds, 96),
        no_configuration,
    )
    .await;
    for quality in [StreamQuality::DataSaver, StreamQuality::High] {
        let audio = open(&library, request(quality, 0)).await.unwrap();
        let QualityAudio::Original { audio, reason } = audio else {
            panic!("a small lossy source must not be converted");
        };
        assert_eq!(reason, "lossy-within-target");
        let OpenedAudio::Content(mut content) = audio else {
            panic!("the original must open");
        };
        let mut body = content.body.take().unwrap();
        let mut length = 0;
        while let Some(chunk) = body.chunk().await.unwrap() {
            length += chunk.len();
        }
        assert_eq!(Some(length as u64), content.length);
    }
}

#[tokio::test]
async fn a_lossy_source_above_the_target_is_converted() {
    let library = library(
        5,
        "mp3",
        |path, seconds| tones::mp3(path, seconds, 320),
        no_configuration,
    )
    .await;
    assert!(matches!(
        open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .unwrap(),
        QualityAudio::Converted(_)
    ));
    assert!(matches!(
        open(&library, request(StreamQuality::High, 0))
            .await
            .unwrap(),
        QualityAudio::Converted(_)
    ));
}

#[tokio::test]
async fn a_head_request_describes_the_stream_without_starting_a_conversion() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("started");
    let library = library(5, "flac", flac, |config, scratch| {
        config.media_tools.ffmpeg = media_tool::script(
            scratch,
            "ffmpeg",
            &format!(
                "case \"$*\" in *adts*) touch {};; esac\nexec /usr/bin/ffmpeg \"$@\"",
                media_tool::quote(&marker)
            ),
        );
    })
    .await;
    let mut head = request(StreamQuality::DataSaver, 0);
    head.original.head = true;
    let QualityAudio::Converted(converted) = open(&library, head).await.unwrap() else {
        panic!("the head must describe a converted stream");
    };
    assert!(converted.body.is_none());
    assert!(!marker.exists());
}

#[tokio::test]
async fn a_failing_converter_falls_back_to_the_original_bytes() {
    let library = library(5, "flac", flac, |config, scratch| {
        config.media_tools.ffmpeg = media_tool::script(
            scratch,
            "ffmpeg",
            "case \"$*\" in *adts*) exit 1;; esac\nexec /usr/bin/ffmpeg \"$@\"",
        );
    })
    .await;
    let audio = open(&library, request(StreamQuality::DataSaver, 0))
        .await
        .unwrap();
    let QualityAudio::Original { reason, .. } = audio else {
        panic!("a failed conversion must serve the original");
    };
    assert_eq!(reason, "conversion-failed");
}

#[tokio::test]
async fn an_unknown_source_is_not_found() {
    let library = library(5, "flac", flac, no_configuration).await;
    let result = library
        .services
        .playback
        .open_quality_audio(
            &library.actor,
            "missing",
            request(StreamQuality::DataSaver, 0),
        )
        .await;
    assert!(matches!(result, Err(Error::NotFound)));
}

#[tokio::test]
async fn uncertain_audio_profiles_preserve_the_original_instead_of_guessing_a_bitrate() {
    for profile in [
        r#"{"streams":[{"codec_name":"mp3"}],"format":{"bit_rate":"320000"}}"#,
        r#"{"streams":[{"codec_name":"unknown","bit_rate":"96000"}]}"#,
    ] {
        let library = library(5, "flac", flac, |config, scratch| {
            config.media_tools.ffprobe = media_tool::script(scratch, "ffprobe", &format!(
                "case \"$*\" in *stream=codec_name,bit_rate:format=bit_rate*) printf '%s' '{}'; exit 0;; esac\nexec /usr/bin/ffprobe \"$@\"", profile));
        }).await;
        let audio = open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .unwrap();
        let QualityAudio::Original { reason, .. } = audio else {
            panic!("an uncertain profile must retain the original");
        };
        assert_eq!(reason, "uncertain-profile");
    }
}
