mod dns;
mod protocol;
mod rejections;
mod tls;

use super::logging::Capture;
use crate::{MediaAllowlist, RealDebridClient, TorrentFileBinding};
use hito_audio::Decoder;
use serde_json::json;
use std::{io::Cursor, time::Duration};
use tls::{API_HOST, Fixture, MEDIA_HOST, Origin};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "private-audio-provider-token-canary";
const SERVICE_TOKEN: &str = "private-audio-service-token-canary";
const CALLER_TOKEN: &str = "private-audio-caller-token-canary";
const TORRENT: &str = "private-audio-torrent-canary";
const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const FILE: &str = "/private-audio-directory/private-audio-recording-canary.wav";
const TITLE: &str = "private-audio-title-canary";
const HOSTER_LINK: &str = "https://hoster.hito-fixture.invalid/private-original-link?signature=private-original-signature&part=one+two";
const MEDIA_TARGET: &str = "/private-media-canary.wav?signature=private-media-signature-canary";
const EXPECTED_FORM: &str = "link=https%3A%2F%2Fhoster.hito-fixture.invalid%2Fprivate-original-link%3Fsignature%3Dprivate-original-signature%26part%3Done%2Btwo";
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAMES: u64 = 96_000;
const WAV_BYTES: usize = 384_044;

#[tokio::test]
async fn opens_and_decodes_the_exact_bound_pcm16_wav_through_owned_tls_without_exposing_urls() {
    let wave = wave();
    let download = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
    let completed = serde_json::to_vec(&json!({
        "id": TORRENT, "filename": TITLE, "hash": HASH, "bytes": WAV_BYTES,
        "progress": 100, "status": "downloaded",
        "files": [{"id": 7, "path": FILE, "bytes": WAV_BYTES, "selected": 1}],
        "links": [HOSTER_LINK]
    }))
    .expect("the owned audio metadata must serialize");
    let unrestricted = serde_json::to_vec(&json!({
        "id": "private-audio-download-id-canary", "filename": TITLE,
        "filesize": WAV_BYTES, "link": HOSTER_LINK, "download": download,
        "mimeType": "audio/wav", "streamable": 1
    }))
    .expect("the owned audio unrestriction must serialize");
    let fixture = Fixture::start(vec![completed, unrestricted], wave.clone()).await;
    let binding = TorrentFileBinding {
        torrent_id: TORRENT.into(),
        info_hash: HASH.into(),
        file_id: 7,
        file_path: FILE.into(),
        file_bytes: WAV_BYTES as u64,
    };
    let original_binding = binding.clone();
    let capture = Capture::default();
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
            .map_err(|_| "the explicit owned media allowlist must initialize")?;
        let client = RealDebridClient::with_test_tls(
            TOKEN.into(),
            fixture.ca_der(),
            fixture.api_address,
            (MEDIA_HOST, fixture.media_address),
        )
        .map_err(|_| "the fixed-origin owned TLS client must initialize")?;
        let mut audio = tokio::time::timeout(
            Duration::from_secs(10),
            client.open_bound_audio(&binding, &allowlist, crate::AudioKind::Pcm16Wav),
        )
        .await
        .map_err(|_| "the owned audio opening must finish within its guard")?
        .map_err(|_| "the exact bound audio must open")?;
        let returned_binding = audio.binding.clone();
        let length = audio.length;
        let crate::AudioFormat::Pcm16Wav(wav_format) = audio.format else {
            panic!("the unchanged WAV policy must return a WAV descriptor");
        };
        let format = (
            wav_format.sample_rate,
            wav_format.channels,
            wav_format.frames,
        );
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(2), audio.next_chunk())
                .await
                .map_err(|_| "the owned audio pull must finish within its idle guard")?
                .map_err(|_| "the owned audio body must remain readable")?;
            let Some(chunk) = chunk else {
                break;
            };
            if chunk.is_empty()
                || chunk.len() > 65_536
                || chunk.len() > WAV_BYTES.saturating_sub(bytes.len())
            {
                return Err("the owned audio pull exceeded its representation bound");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((returned_binding, length, format, bytes))
    })
    .with_subscriber(capture.dispatch())
    .await;
    let received = fixture.close().await;

    let (returned_binding, length, format, bytes) = result
        .expect("the complete owned audio workflow must finish")
        .expect("the bound audio opening and consumption must succeed");
    assert!(
        binding == original_binding && returned_binding == original_binding,
        "audio opening must preserve the immutable exact binding"
    );
    assert_eq!(length, WAV_BYTES as u64);
    assert_eq!(format, (SAMPLE_RATE, CHANNELS, FRAMES));
    assert!(
        bytes == wave,
        "qualification must retain the exact WAV prefix and complete media body"
    );
    assert_eq!(
        received.len(),
        3,
        "audio opening must make only one metadata GET, one unrestriction POST and one direct media GET"
    );
    for (index, method, target) in [
        (0, "GET", format!("/rest/1.0/torrents/info/{TORRENT}")),
        (1, "POST", "/rest/1.0/unrestrict/link".into()),
    ] {
        let observed = &received[index];
        assert!(
            observed.origin == Origin::Api && observed.server_name == API_HOST,
            "provider API requests must retain the fixed HTTPS hostname and TLS SNI"
        );
        let request = &observed.request;
        assert!(
            request.method == method && request.target == target,
            "the API request must remain exactly bound without retries or extra operations"
        );
        assert!(request.header("host") == Some(API_HOST));
        assert!(
            request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()),
            "only the provider API may receive the provider credential"
        );
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(request.header("accept-encoding"), Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("proxy-authorization").is_none());
        assert!(request.header("x-hito-plugin-protocol").is_none());
    }
    assert!(received[0].request.body.is_empty());
    assert!(
        received[1].request.body.as_slice() == EXPECTED_FORM.as_bytes(),
        "only the private selected hoster link may be form encoded"
    );
    assert_eq!(
        received[1].request.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert!(
        received[1].request.header("content-length")
            == Some(EXPECTED_FORM.len().to_string().as_str())
    );
    let media = &received[2];
    assert!(
        media.origin == Origin::Media && media.server_name == MEDIA_HOST,
        "media must use its explicit allowed hostname and verified TLS SNI"
    );
    assert!(
        media.request.method == "GET" && media.request.target == MEDIA_TARGET,
        "the private candidate must be consumed directly without redirects or retries"
    );
    assert!(media.request.header("host") == Some(MEDIA_HOST));
    assert!(media.request.header("authorization").is_none());
    assert!(media.request.header("cookie").is_none());
    assert!(media.request.header("proxy-authorization").is_none());
    assert!(media.request.header("x-hito-plugin-protocol").is_none());
    assert!(media.request.header("range").is_none());
    assert_eq!(media.request.header("accept-encoding"), Some("identity"));
    assert!(media.request.body.is_empty());
    for observed in &received {
        assert!(observed.request.header("transfer-encoding").is_none());
        for (_, value) in &observed.request.headers {
            assert!(
                !value.contains(SERVICE_TOKEN) && !value.contains(CALLER_TOKEN),
                "service and caller credentials must never enter provider or media transport"
            );
        }
    }

    let mut decoder =
        Decoder::open(Cursor::new(bytes)).expect("the complete delivered WAV must decode");
    assert_eq!(decoder.format().sample_rate, SAMPLE_RATE);
    assert_eq!(decoder.format().channels, CHANNELS);
    let mut samples = 0_usize;
    while let Some(block) = decoder
        .read()
        .expect("the delivered WAV must decode through EOF")
    {
        for sample in block.samples {
            let expected = f32::from(pcm_sample(samples)) / 32768.0;
            assert!(
                (*sample - expected).abs() < 0.000_001,
                "the decoder must receive the exact owned PCM samples"
            );
            samples += 1;
        }
    }
    assert_eq!(samples as u64, FRAMES * u64::from(CHANNELS));
    let logs = capture.text();
    for required in [
        "request_id",
        "operation",
        "outcome",
        "elapsed_ms",
        "complete",
    ] {
        assert!(
            logs.contains(required),
            "opening and consumed-body logs must be active and structured"
        );
    }
    let completed_bodies = logs
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("the owned audio log must be JSON")
        })
        .filter(|event| {
            event["fields"]["event"] == "remote_audio_body_finished"
                && event["fields"]["outcome"] == "complete"
                && event["fields"]["bytes"].as_u64() == Some(WAV_BYTES as u64)
        })
        .count();
    assert_eq!(
        completed_bodies, 1,
        "the fully consumed owned body must log one safe completion with its byte count"
    );
    for private in [
        TOKEN,
        SERVICE_TOKEN,
        CALLER_TOKEN,
        TORRENT,
        HASH,
        FILE,
        TITLE,
        HOSTER_LINK,
        download.as_str(),
        EXPECTED_FORM,
        "private-",
        "http://",
        "https://",
    ] {
        assert!(
            !logs.contains(private),
            "audio logs must exclude private binding, URL, credential and response values"
        );
    }
}

fn pcm_sample(index: usize) -> i16 {
    let sample = (((index / usize::from(CHANNELS)) % 257) as i16 - 128) * 128;
    if index.is_multiple_of(usize::from(CHANNELS)) {
        sample
    } else {
        -sample
    }
}

fn wave() -> Vec<u8> {
    let data_bytes = (FRAMES * u64::from(CHANNELS) * 2) as u32;
    let mut bytes = Vec::with_capacity(WAV_BYTES);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(data_bytes + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&CHANNELS.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    bytes.extend_from_slice(&(CHANNELS * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..(FRAMES * u64::from(CHANNELS)) as usize {
        bytes.extend_from_slice(&pcm_sample(index).to_le_bytes());
    }
    bytes
}
