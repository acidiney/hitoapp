use super::fixture::*;
use hito_core::Error;
use std::time::Duration;

#[tokio::test]
async fn failed_encoder_exit_is_never_successful_eof_after_valid_samples() {
    let library = scripted("printf '\\000\\000\\200\\076'; exit 7").await;
    let mut stream = library
        .grant()
        .await
        .open_pcm(Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(stream.next_block().await.unwrap().unwrap().samples, [0.25]);
    assert!(matches!(stream.next_block().await, Err(Error::Upstream(_))));
    stream.close().await.unwrap();
}

#[tokio::test]
async fn invalid_pcm_and_partial_frames_fail_without_a_successful_empty_stream() {
    for body in [
        "printf '\\000'; exit 0",
        "printf '\\000\\000\\300\\177'; exit 0",
        "exit 0",
    ] {
        let library = scripted(body).await;
        assert!(matches!(
            library.grant().await.open_pcm(Duration::ZERO).await,
            Err(Error::Upstream(_))
        ));
    }
}

#[tokio::test]
async fn a_missing_configured_decoder_is_distinct_from_unsupported_or_corrupt_audio() {
    let configured = std::sync::Mutex::new(None);
    let library = library(2, |config, tools| {
        let program = hito_core::test_support::media_tool::script(
            tools,
            "ffmpeg",
            "exec /usr/bin/ffmpeg \"$@\"",
        );
        config.media_tools.ffmpeg = program.clone();
        *configured.lock().unwrap() = Some(program);
    })
    .await;
    std::fs::remove_file(configured.into_inner().unwrap().unwrap()).unwrap();
    assert!(matches!(
        library.grant().await.open_pcm(Duration::ZERO).await,
        Err(Error::MediaToolsMissing)
    ));
}

#[tokio::test]
async fn failed_and_malformed_probes_are_not_mistaken_for_unsupported_profiles() {
    for body in [
        "printf '%s' 'not json'; exit 0",
        "printf '%s' '{\"streams\":[{\"codec_name\":\"opus\",\"sample_rate\":\"48000\",\"channels\":1}]}'; exit 7",
    ] {
        let library = library(2, |config, tools| {
            config.media_tools.ffprobe = hito_core::test_support::media_tool::script(tools, "ffprobe", &format!("case \"$*\" in *stream=codec_name,sample_rate,channels,duration*) {body};; esac\nexec /usr/bin/ffprobe \"$@\""));
        }).await;
        assert!(matches!(
            library.grant().await.open_pcm(Duration::ZERO).await,
            Err(Error::Upstream(_))
        ));
    }
}
