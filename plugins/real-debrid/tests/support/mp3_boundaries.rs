use crate::{
    FILE, boundary,
    boundary::Fixture,
    decode, fixture,
    support::{self, MEDIA_HOST, SERVICE_TOKEN, SOURCE, media::MediaGate, provider::Counts},
};
use futures_util::{FutureExt, poll};
use hito_debrid_real_debrid::AudioKind;
use hito_plugin_real_debrid::CuratedAudioRecording;
use reqwest::{Method, StatusCode};
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};

const LABEL: &str = "Owned encoded audio";

#[tokio::test]
async fn mp3_declaration_rejects_wav_bytes_despite_mp3_filename_and_mime() {
    let mut wav = support::audio::wav();
    wav.truncate(44 + 4096);
    wav[4..8].copy_from_slice(&4132_u32.to_le_bytes());
    wav[40..44].copy_from_slice(&4096_u32.to_le_bytes());
    let mut decoder = hito_audio::Decoder::open(std::io::Cursor::new(wav.clone()))
        .expect("owned short PCM16 WAV must decode");
    let mut samples = 0;
    while let Some(block) = decoder.read().expect("owned WAV must reach native EOF") {
        samples += block.samples.len();
    }
    assert_eq!(samples, 2048);
    reject(wav, "audio/mpeg", FILE, AudioKind::Mp3).await;
}

#[tokio::test]
async fn wav_declaration_rejects_native_mp3_despite_wav_filename_and_mime() {
    let encoded = fixture().await;
    decode(&encoded);
    reject(encoded, "audio/wav", support::FILE, AudioKind::Pcm16Wav).await;
}

#[tokio::test]
async fn malformed_id3_and_incomplete_or_inconsistent_frames_reject_before_audio() {
    let encoded = fixture().await;
    let expected_pcm = decode(&encoded);
    let second = audio_start(&encoded) + 384;
    assert_eq!(encoded[second], 0xff, "owned 128k/48k MPEG frame boundary");

    let mut invalid_size = encoded.clone();
    invalid_size[6] |= 0x80;
    let oversized = pad_id3(&encoded, 61_441);
    assert!(
        decode(&oversized) == expected_pcm,
        "oversized ID3 fixture must remain natively decodable"
    );
    let mut reserved_flag = encoded.clone();
    reserved_flag[5] |= 1;
    let incomplete = encoded[..second + 4].to_vec();
    let mut inconsistent = encoded;
    inconsistent[second + 2] &= !0x0c;
    for bytes in [
        invalid_size,
        oversized,
        reserved_flag,
        incomplete,
        inconsistent,
    ] {
        reject(bytes, "audio/mpeg", FILE, AudioKind::Mp3).await;
    }
}

#[tokio::test]
async fn partial_id3_and_frame_headers_hold_http_opening_until_original_mp3_is_ready() {
    let original = fixture().await;
    let expected_pcm = decode(&original);
    let encoded = pad_id3(&original, 61_440);
    assert!(
        decode(&encoded) == expected_pcm,
        "maximum accepted ID3 padding must preserve native PCM"
    );
    let start = audio_start(&encoded);
    let gate = MediaGate::new(vec![3, start + 3, start + 384 + 3], encoded.len());
    let fixture = start_fixture(
        encoded.clone(),
        "audio/mpeg",
        FILE,
        AudioKind::Mp3,
        Some(gate.clone()),
    )
    .await;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(15), async {
        head(&fixture, AudioKind::Mp3, encoded.len()).await;
        let opening = fixture.request(Method::GET, &audio_path())
            .header("x-hito-plugin-protocol", "1").send();
        tokio::pin!(opening);
        for _ in 0..3 {
            tokio::select! {
                biased;
                _ = &mut opening => panic!("HTTP audio must not publish while its required prefix is withheld"),
                _ = gate.paused() => {}
            }
            assert!(poll!(opening.as_mut()).is_pending(), "partial-prefix HTTP opening must remain pending");
            gate.release();
        }
        let mut response = opening.await.expect("complete qualified prefix must release HTTP opening");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "audio/mpeg");
        assert_eq!(response.headers()["content-length"].to_str().expect("audio length")
            .parse::<usize>().expect("decimal audio length"), encoded.len());
        let mut received = Vec::new();
        while let Some(part) = response.chunk().await.expect("fragmented original audio must reach EOF") {
            assert!(part.len() <= encoded.len().saturating_sub(received.len()));
            received.extend_from_slice(&part);
        }
        assert!(received == encoded, "fragmented transport must retain every original byte");
        assert!(decode(&received) == expected_pcm, "fragmented transport must retain native PCM through EOF");
        assert_eq!(fixture.provider.counts(), Counts { observations: 2, posts: 1, media: 1 });
    })).catch_unwind().await;
    gate.release_all();
    let (capture, exchanges) = fixture.close().await;
    match outcome {
        Ok(result) => result.expect("fragmented audio must finish within its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    assert_eq!(exchanges.len(), 4);
    capture.assert_private_completion(&private_canaries(), encoded.len() as u64);
}

async fn reject(bytes: Vec<u8>, mime: &'static str, path: &str, kind: AudioKind) {
    let length = bytes.len();
    let fixture = start_fixture(bytes, mime, path, kind, None).await;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(10), async {
        head(&fixture, kind, length).await;
        boundary::rejected(
            fixture
                .request(Method::GET, &audio_path())
                .header("x-hito-plugin-protocol", "1"),
            StatusCode::BAD_GATEWAY,
            "provider_unavailable",
        )
        .await;
        assert_eq!(
            fixture.provider.counts(),
            Counts {
                observations: 2,
                posts: 1,
                media: 1
            }
        );
    }))
    .catch_unwind()
    .await;
    let (capture, exchanges) = fixture.close().await;
    match outcome {
        Ok(result) => result.expect("rejected audio must finish within its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    assert_eq!(
        exchanges.len(),
        4,
        "declared-format rejection must follow only one fresh audio attempt"
    );
    capture.assert_private_requests(&private_canaries(), &[200, 502]);
}

async fn start_fixture(
    bytes: Vec<u8>,
    mime: &'static str,
    path: &str,
    kind: AudioKind,
    gate: Option<Arc<MediaGate>>,
) -> Fixture {
    let mut binding = support::binding();
    binding.file_path = path.into();
    binding.file_bytes = bytes.len() as u64;
    let provider = support::provider::Provider::start_audio(bytes, mime, path, gate).await;
    Fixture::with_provider(
        vec![CuratedAudioRecording {
            recording: support::recording(),
            source_id: SOURCE.into(),
            label: LABEL.into(),
            binding,
            audio_kind: kind,
        }],
        provider,
    )
    .await
}

async fn head(fixture: &Fixture, kind: AudioKind, length: usize) {
    let response = fixture
        .request(Method::HEAD, &audio_path())
        .header("x-hito-plugin-protocol", "1")
        .send()
        .await
        .expect("metadata-only HEAD");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], kind.content_type());
    assert_eq!(
        response.headers()["content-length"]
            .to_str()
            .expect("audio length")
            .parse::<usize>()
            .expect("decimal audio length"),
        length
    );
    assert!(response.bytes().await.expect("HEAD body").is_empty());
    assert_eq!(
        fixture.provider.counts(),
        Counts {
            observations: 1,
            posts: 0,
            media: 0
        }
    );
}

fn audio_start(bytes: &[u8]) -> usize {
    assert!(
        bytes.starts_with(b"ID3\x04\x00\x00"),
        "owned encoder must emit the expected ID3v2.4 prefix"
    );
    let size = bytes[6..10]
        .iter()
        .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
    let offset = 10 + size;
    assert!(offset + 772 < bytes.len());
    assert_eq!(bytes[offset], 0xff);
    assert_eq!(
        bytes[offset + 2] & 0xfc,
        0x94,
        "owned encoder must emit 128k/48k MPEG-1 Layer III"
    );
    offset
}

fn pad_id3(original: &[u8], payload_size: usize) -> Vec<u8> {
    let start = audio_start(original);
    assert!((start - 10..=61_441).contains(&payload_size));
    let mut padded = Vec::with_capacity(10 + payload_size + original.len() - start);
    padded.extend_from_slice(&original[..start]);
    for (index, byte) in padded[6..10].iter_mut().enumerate() {
        *byte = ((payload_size >> (21 - index * 7)) & 0x7f) as u8;
    }
    padded.resize(10 + payload_size, 0);
    padded.extend_from_slice(&original[start..]);
    assert!(
        padded[10 + payload_size..] == original[start..],
        "ID3 padding must preserve original audio frames"
    );
    padded
}

fn audio_path() -> String {
    format!("v1/audio?source_id={SOURCE}")
}

fn private_canaries() -> Vec<&'static str> {
    let mut canaries = boundary::private_log_canaries().to_vec();
    canaries.extend([
        FILE,
        LABEL,
        SOURCE,
        support::RECORDING,
        MEDIA_HOST,
        SERVICE_TOKEN,
    ]);
    canaries
}
