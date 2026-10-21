#[path = "support/media.rs"]
mod media;
mod support;

use hito_audio::Decoder;
use hito_debrid_all_debrid::{
    AllDebridClient, DelayedPoll, MagnetFileBinding, MediaAllowlist, PreparedFile,
};
use media::MediaFixture;
use serde_json::json;
use std::{io::Cursor, time::Duration};
use support::{Fixture, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-private-media-provider-key";
const MEDIA_HOST: &str = "media.hito-fixture.invalid";
const MEDIA_TARGET: &str = "/owned-private-media.wav?signature=owned-private-media-signature";
const FRAMES: u64 = 96_000;
const CHANNELS: u16 = 2;
const SAMPLE_RATE: u32 = 48_000;
const WAV_BYTES: u64 = 384_044;

#[tokio::test]
async fn delayed_candidate_opens_the_reobserved_bound_wav_without_exposing_or_reunlocking_it() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let wave = wave();
        let media = MediaFixture::start(wave.clone()).await;
        let link = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
        let binding = MagnetFileBinding {
            magnet_id: 7123,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            magnet_bytes: WAV_BYTES * 2 + 400,
            file_path: vec!["album".into(), "owned.wav".into()],
            file_bytes: WAV_BYTES,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let status = serde_json::to_vec(&json!({
            "status": "success", "data": { "magnets": [{
                "id": 7123, "size": WAV_BYTES * 2 + 400, "statusCode": 4
            }] }
        }))
        .unwrap();
        let files = serde_json::to_vec(&json!({
            "status": "success", "data": { "magnets": [{
                "id": "7123", "files": [
                    { "n": "other", "e": [{ "n": "owned.wav", "s": WAV_BYTES,
                        "l": "https://alldebrid.com/f/owned-decoy" }] },
                    { "n": "album", "e": [{ "n": "owned.wav", "s": WAV_BYTES,
                        "l": "https://alldebrid.com/f/owned-selected" }] },
                    { "n": "readme.txt", "s": 400, "l": "https://alldebrid.com/f/owned-note" }
                ]
            }] }
        }))
        .unwrap();
        let fixture = Fixture::start(vec![
            status.clone(),
            files.clone(),
            serde_json::to_vec(&json!({
                "status": "success", "data": { "delayed": 2468, "filesize": WAV_BYTES }
            }))
            .unwrap(),
            serde_json::to_vec(&json!({
                "status": "success", "data": { "status": 2, "time_left": 0, "link": link }
            }))
            .unwrap(),
            status,
            files,
        ])
        .await;
        let capture = Capture::default();
        let dispatch = capture.dispatch();
        let client = AllDebridClient::with_test_tls_media(
            TOKEN.into(),
            fixture.ca_der(),
            fixture.address(),
            media.ca_der(),
            media.address(),
        )
        .expect("the owned API and media TLS peers must initialize");
        let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")])
            .expect("the exact owned media origin must initialize");
        let PreparedFile::Delayed(mut receipt) = client
            .prepare_bound_file(&binding)
            .with_subscriber(dispatch.clone())
            .await
            .expect("the exact file must return one delayed ACK")
        else {
            panic!("the owned delayed acknowledgment must remain delayed")
        };
        let candidate = match client
            .poll_delayed(&mut receipt)
            .with_subscriber(dispatch.clone())
            .await
            .expect("the exact delayed ID must return an opaque candidate")
        {
            DelayedPoll::Candidate(candidate) => candidate,
            DelayedPoll::Processing { .. } => panic!("the owned provider reported completion"),
        };
        capture.assert_workflows(
            &[
                ("bound_file_preparation", "delayed", 1),
                ("delayed_file_poll", "candidate", 1),
            ],
            &[
                TOKEN,
                &binding.info_hash,
                "album",
                "owned.wav",
                &link,
                "owned-selected",
            ],
        );
        let media_capture = Capture::default();
        let mut opened = client
            .open_bound_audio(candidate, &allowlist)
            .with_subscriber(media_capture.dispatch())
            .await
            .expect("the current exact binding must open its owned WAV");
        assert!(opened.binding == binding);
        assert_eq!(opened.length, WAV_BYTES);
        let hito_remote_audio::AudioFormat::Pcm16Wav(wav_format) = opened.format else {
            panic!("the unchanged WAV policy must return a WAV descriptor");
        };
        assert_eq!(
            (
                wav_format.sample_rate,
                wav_format.channels,
                wav_format.frames
            ),
            (SAMPLE_RATE, CHANNELS, FRAMES)
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = opened
            .next_chunk()
            .await
            .expect("the WAV must remain valid through EOF")
        {
            assert!(!chunk.is_empty() && chunk.len() <= 65_536);
            assert!(chunk.len() <= (WAV_BYTES as usize).saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk);
        }
        assert!(bytes == wave);
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        let requests = fixture.close().await;
        let media_requests = media.close().await;
        assert_eq!(requests.len(), 6);
        assert_eq!(media_requests.len(), 1);
        assert!(
            requests[3]
                .received_at
                .duration_since(requests[2].received_at)
                >= Duration::from_secs(5)
        );
        assert!(media_requests[0].received_at >= requests[5].received_at);
        for (index, request) in requests.iter().enumerate() {
            assert!(request.method == "POST");
            assert!(request.server_name == "api.alldebrid.com");
            assert!(request.header("host") == Some("api.alldebrid.com"));
            assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            assert!(request.header("content-type") == Some("application/x-www-form-urlencoded"));
            assert!(request.header("accept-encoding") == Some("identity"));
            for name in ["cookie", "referer", "range", "proxy-authorization"] {
                assert!(request.header(name).is_none());
            }
            let (target, body) = match index {
                0 | 4 => ("/v4.1/magnet/status", "id=7123"),
                1 | 5 => ("/v4/magnet/files", "id%5B%5D=7123"),
                2 => (
                    "/v4/link/unlock",
                    "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-selected",
                ),
                3 => ("/v4/link/delayed", "id=2468"),
                _ => unreachable!(),
            };
            assert!(request.target == target && request.body == body.as_bytes());
        }
        let media = &media_requests[0];
        assert!(
            media.method == "GET"
                && media.target == MEDIA_TARGET
                && media.server_name == MEDIA_HOST
        );
        assert!(media.header("host") == Some(MEDIA_HOST));
        assert!(media.header("accept") == Some("audio/wav"));
        assert!(media.header("accept-encoding") == Some("identity"));
        assert!(media.body.is_empty());
        for name in [
            "authorization",
            "cookie",
            "referer",
            "range",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(media.header(name).is_none());
        }
        let mut decoder = Decoder::open(Cursor::new(bytes)).expect("the qualified WAV must decode");
        let mut samples = 0;
        while let Some(block) = decoder.read().expect("the decoder must reach exact EOF") {
            for sample in block.samples {
                let expected = f32::from(pcm_sample(samples)) / 32768.0;
                assert!((*sample - expected).abs() < 0.000_001);
                samples += 1;
            }
        }
        assert_eq!(samples as u64, FRAMES * u64::from(CHANNELS));
        assert_audio_logs(
            &media_capture,
            &[
                TOKEN,
                &binding.info_hash,
                "album",
                "owned.wav",
                &link,
                "owned-selected",
                "owned-private-media-signature",
            ],
            WAV_BYTES,
        );
    })
    .await
    .expect("the exact owned delayed-to-WAV workflow exceeded its total test bound");
}

#[tokio::test]
async fn direct_ack_candidate_opens_only_the_freshly_bound_wav_without_polling_or_reunlocking() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let wave = wave();
        let media = MediaFixture::start(wave.clone()).await;
        let link = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
        let binding = MagnetFileBinding {
            magnet_id: 7123,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            magnet_bytes: WAV_BYTES * 2 + 400,
            file_path: vec!["album".into(), "owned.wav".into()],
            file_bytes: WAV_BYTES,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let status = serde_json::to_vec(&json!({
            "status": "success", "data": { "magnets": [{
                "id": 7123, "size": WAV_BYTES * 2 + 400, "statusCode": 4
            }] }
        }))
        .unwrap();
        let files = serde_json::to_vec(&json!({
            "status": "success", "data": { "magnets": [{
                "id": "7123", "files": [
                    { "n": "other", "e": [{ "n": "owned.wav", "s": WAV_BYTES,
                        "l": "https://alldebrid.com/f/owned-direct-decoy" }] },
                    { "n": "album", "e": [{ "n": "owned.wav", "s": WAV_BYTES,
                        "l": "https://alldebrid.com/f/owned-direct-selected" }] },
                    { "n": "readme.txt", "s": 400,
                        "l": "https://alldebrid.com/f/owned-direct-note" }
                ]
            }] }
        }))
        .unwrap();
        let fixture = Fixture::start(vec![
            status.clone(),
            files.clone(),
            serde_json::to_vec(&json!({
                "status": "success", "data": {
                    "link": link, "filesize": WAV_BYTES,
                    "id": "owned-private-direct-generation",
                    "filename": "owned-private-direct-provider-title.wav"
                }
            }))
            .unwrap(),
            status,
            files,
        ])
        .await;
        let client = AllDebridClient::with_test_tls_media(
            TOKEN.into(),
            fixture.ca_der(),
            fixture.address(),
            media.ca_der(),
            media.address(),
        )
        .expect("the owned API and media TLS peers must initialize");
        let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap();
        let preparation = Capture::default();
        let PreparedFile::Candidate(candidate) = client
            .prepare_bound_file(&binding)
            .with_subscriber(preparation.dispatch())
            .await
            .expect("the exact direct ACK must retain an opaque bound candidate")
        else {
            panic!("an immediate generated acknowledgment must not fabricate a delayed receipt")
        };
        assert!(candidate.binding() == &binding);
        let capture = Capture::default();
        let mut opened = client
            .open_bound_audio(candidate, &allowlist)
            .with_subscriber(capture.dispatch())
            .await
            .expect("the direct candidate must use the common fresh-binding media policy");
        assert!(opened.binding == binding);
        assert_eq!(opened.length, WAV_BYTES);
        let hito_remote_audio::AudioFormat::Pcm16Wav(wav_format) = opened.format else {
            panic!("the unchanged WAV policy must return a WAV descriptor");
        };
        assert_eq!(
            (
                wav_format.sample_rate,
                wav_format.channels,
                wav_format.frames
            ),
            (SAMPLE_RATE, CHANNELS, FRAMES)
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = opened
            .next_chunk()
            .await
            .expect("the direct candidate body must remain valid through exact EOF")
        {
            assert!(!chunk.is_empty() && chunk.len() <= 65_536);
            assert!(chunk.len() <= (WAV_BYTES as usize).saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk);
        }
        assert!(bytes == wave);
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        let requests = fixture.close().await;
        let media_requests = media.close().await;
        assert_eq!(requests.len(), 5);
        assert_eq!(media_requests.len(), 1);
        for (index, request) in requests.iter().enumerate() {
            assert!(request.method == "POST" && request.server_name == "api.alldebrid.com");
            assert!(request.header("host") == Some("api.alldebrid.com"));
            assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            assert!(request.header("content-type") == Some("application/x-www-form-urlencoded"));
            assert!(request.header("accept-encoding") == Some("identity"));
            for name in ["cookie", "referer", "range", "proxy-authorization"] {
                assert!(request.header(name).is_none());
            }
            let (target, body) = match index {
                0 | 3 => ("/v4.1/magnet/status", "id=7123"),
                1 | 4 => ("/v4/magnet/files", "id%5B%5D=7123"),
                2 => (
                    "/v4/link/unlock",
                    "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-direct-selected",
                ),
                _ => unreachable!(),
            };
            assert!(request.target == target && request.body == body.as_bytes());
        }
        let media = &media_requests[0];
        assert!(
            media.method == "GET"
                && media.target == MEDIA_TARGET
                && media.server_name == MEDIA_HOST
                && media.body.is_empty()
        );
        assert!(media.header("host") == Some(MEDIA_HOST));
        assert!(media.header("accept") == Some("audio/wav"));
        assert!(media.header("accept-encoding") == Some("identity"));
        assert!(media.received_at >= requests[4].received_at);
        for name in [
            "authorization",
            "cookie",
            "referer",
            "range",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(media.header(name).is_none());
        }
        let mut decoder = Decoder::open(Cursor::new(bytes)).unwrap();
        let mut samples = 0;
        while let Some(block) = decoder
            .read()
            .expect("the direct WAV must decode through EOF")
        {
            for sample in block.samples {
                let expected = f32::from(pcm_sample(samples)) / 32768.0;
                assert!((*sample - expected).abs() < 0.000_001);
                samples += 1;
            }
        }
        assert_eq!(samples as u64, FRAMES * u64::from(CHANNELS));
        let private = [
            TOKEN,
            &binding.info_hash,
            "album",
            "owned.wav",
            &link,
            "owned-direct-selected",
            "owned-private-direct-generation",
            "owned-private-direct-provider-title.wav",
            "owned-private-media-signature",
        ];
        preparation.assert_workflows(&[("bound_file_preparation", "candidate", 1)], &private);
        assert_audio_logs(&capture, &private, WAV_BYTES);
    })
    .await
    .expect("the bounded direct-ACK audio workflow exceeded its deadline");
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
    let mut bytes = Vec::with_capacity(WAV_BYTES as usize);
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

fn assert_audio_logs(capture: &Capture, private: &[&str], length: u64) {
    let text = capture.text();
    for secret in private.iter().copied().chain([
        "http://",
        "https://",
        "http%3A",
        "https%3A",
        "Bearer ",
        "owned-private-media-cookie",
        "owned-private-upstream-name.wav",
    ]) {
        let escaped = serde_json::to_string(secret).unwrap();
        assert!(!text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]));
    }
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let expected = [
        (
            "hito_debrid_all_debrid::client",
            "all_debrid_observation_finished",
            "provider_completed",
            "magnet_observation",
            1,
        ),
        (
            "hito_debrid_all_debrid::client",
            "all_debrid_audio_opening_finished",
            "complete",
            "bound_audio_opening",
            1,
        ),
        (
            "hito_remote_audio::body",
            "remote_audio_body_finished",
            "complete",
            "bound_audio_opening",
            length,
        ),
    ];
    assert_eq!(events.len(), expected.len());
    let mut correlations = std::collections::BTreeSet::new();
    let mut opening = None;
    for (entry, (target, event, outcome, operation, count)) in events.iter().zip(expected) {
        assert_keys(entry, &["level", "fields", "target", "span", "spans"]);
        assert_eq!(entry["level"], "DEBUG");
        assert_eq!(entry["target"], target);
        let fields = &entry["fields"];
        assert_keys(
            fields,
            if target == "hito_remote_audio::body" {
                &["event", "message", "outcome", "bytes", "elapsed_ms"]
            } else {
                &["event", "outcome", "count", "elapsed_ms"]
            },
        );
        assert_eq!(fields["event"], event);
        assert_eq!(fields["outcome"], outcome);
        assert_eq!(
            fields[if target == "hito_remote_audio::body" {
                "bytes"
            } else {
                "count"
            }]
            .as_u64(),
            Some(count)
        );
        assert!(fields["elapsed_ms"].as_u64().is_some());
        let spans = entry["spans"].as_array().unwrap();
        assert_eq!(
            spans.len(),
            if operation == "magnet_observation" {
                2
            } else {
                1
            }
        );
        let current = spans.last().unwrap();
        assert_eq!(entry["span"], *current);
        for span in spans {
            assert_keys(span, &["name", "operation", "request_id"]);
            let name = match span["operation"].as_str().unwrap() {
                "magnet_observation" => "all_debrid_observation",
                "bound_audio_opening" => "all_debrid_audio",
                _ => panic!("The media workflow must use only its known operation spans"),
            };
            assert_eq!(span["name"], name);
        }
        assert_eq!(current["operation"], operation);
        let correlation = uuid::Uuid::parse_str(current["request_id"].as_str().unwrap()).unwrap();
        assert_eq!(correlation.get_version_num(), 4);
        if target == "hito_remote_audio::body" {
            assert_eq!(Some(correlation), opening);
            assert_eq!(fields["message"], "Remote audio body finished");
        } else {
            assert!(correlations.insert(correlation));
            if operation == "bound_audio_opening" {
                opening = Some(correlation);
            }
        }
        if operation == "magnet_observation" {
            assert_eq!(spans[0]["operation"], "bound_audio_opening");
            assert_eq!(spans[0]["request_id"], events[1]["span"]["request_id"]);
        }
    }
    assert_eq!(correlations.len(), 2);
}

fn assert_keys(value: &serde_json::Value, expected: &[&str]) {
    let actual = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected.iter().copied().collect());
}
