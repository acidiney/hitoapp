#[path = "support/media.rs"]
mod media;
mod support;

use hito_debrid_all_debrid::{
    AllDebridClient, DelayedFileReceipt, DelayedPoll, MagnetFileBinding, MediaAllowlist,
};
use media::MediaFixture;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use support::{Fixture, Reply, logs::Capture};
use tokio::sync::Notify;
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-private-opening-cancellation-key";
const MEDIA_HOST: &str = "media.hito-fixture.invalid";
const MEDIA_TARGET: &str = "/owned-cancellation.wav?signature=owned-private-cancellation-signature";
const WAV_BYTES: u64 = 384_044;

#[tokio::test]
async fn dropping_a_held_open_outside_its_dispatch_preserves_logs_and_recovers_both_media_slots() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let wave = silent_wave();
        let media = MediaFixture::start(wave.clone()).await;
        let link = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
        let binding = MagnetFileBinding {
            magnet_id: 7123,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            magnet_bytes: WAV_BYTES,
            file_path: vec![
                "private-cancellation-album".into(),
                "owned-cancellation.wav".into(),
            ],
            file_bytes: WAV_BYTES,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let mut pending: DelayedFileReceipt = serde_json::from_value(json!({
            "binding": binding, "delayed_id": 2468
        }))
        .expect("the trusted owned pending snapshot must reopen");
        let candidate = serde_json::to_vec(&json!({
            "status": "success", "data": { "status": 2, "time_left": 0, "link": link }
        }))
        .unwrap();
        let status = serde_json::to_vec(&json!({
            "status": "success", "data": { "magnets": [{
                "id": 7123, "size": WAV_BYTES, "statusCode": 4
            }] }
        }))
        .unwrap();
        let files = serde_json::to_vec(&json!({
            "status": "success", "data": { "magnets": [{
                "id": "7123", "files": [{ "n": "private-cancellation-album", "e": [{
                    "n": "owned-cancellation.wav", "s": WAV_BYTES,
                    "l": "https://alldebrid.com/f/owned-private-cancellation-leaf"
                }] }]
            }] }
        }))
        .unwrap();
        let received = Arc::new(Notify::new());
        let retired = Arc::new(Notify::new());
        let fixture = Fixture::start(vec![
            Reply::Json(candidate.clone()),
            Reply::HoldUntilDisconnectObserved {
                received: received.clone(),
                retired: retired.clone(),
            },
            Reply::Json(candidate.clone()),
            Reply::Json(status.clone()),
            Reply::Json(files.clone()),
            Reply::Json(candidate),
            Reply::Json(status),
            Reply::Json(files),
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
        let polls = Capture::default();
        let owner = Capture::default();
        let decoy = Capture::default();
        let recovery = Capture::default();
        let DelayedPoll::Candidate(candidate) = client
            .poll_delayed(&mut pending)
            .with_subscriber(polls.dispatch())
            .await
            .unwrap()
        else {
            panic!("the owned delayed response must contain a candidate")
        };
        let mut held = Box::pin(
            client
                .open_bound_audio(candidate, &allowlist)
                .with_subscriber(owner.dispatch()),
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::select! {
                _ = received.notified() => {},
                _ = &mut held => panic!("fresh status must remain held at the real provider"),
            }
        })
        .await
        .expect("the actual opening must reach the owned provider");
        tracing::dispatcher::with_default(&decoy.dispatch(), || drop(held));
        tokio::time::timeout(Duration::from_secs(2), retired.notified())
            .await
            .expect("dropping the opening must retire the actual TLS peer before recovery");

        let mut bodies = Vec::new();
        for _ in 0..2 {
            let DelayedPoll::Candidate(candidate) = client
                .poll_delayed(&mut pending)
                .with_subscriber(polls.dispatch())
                .await
                .unwrap()
            else {
                panic!("the explicit recovery poll must retain an opaque candidate")
            };
            let body = client
                .open_bound_audio(candidate, &allowlist)
                .with_subscriber(recovery.dispatch())
                .await
                .expect("both media permits must recover after cancellation");
            assert!(body.binding == binding);
            assert_eq!(body.length, WAV_BYTES);
            bodies.push(body);
        }
        assert_eq!(
            bodies.len(),
            2,
            "both bodies must coexist before either reaches EOF"
        );
        for mut body in bodies {
            let mut bytes = Vec::new();
            while let Some(chunk) = body.next_chunk().await.unwrap() {
                assert!(!chunk.is_empty() && chunk.len() <= 65_536);
                assert!(chunk.len() <= (WAV_BYTES as usize).saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk);
            }
            assert!(bytes == wave);
        }
        let requests = fixture.close().await;
        let media_requests = media.close().await;
        assert_eq!(requests.len(), 8);
        assert_eq!(media_requests.len(), 2);
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
                0 | 2 | 5 => ("/v4/link/delayed", "id=2468"),
                1 | 3 | 6 => ("/v4.1/magnet/status", "id=7123"),
                4 | 7 => ("/v4/magnet/files", "id%5B%5D=7123"),
                _ => unreachable!(),
            };
            assert!(request.target == target && request.body == body.as_bytes());
        }
        let poll_requests = [&requests[0], &requests[2], &requests[5]];
        for pair in poll_requests.windows(2) {
            assert!(
                pair[1].received_at.duration_since(pair[0].received_at) >= Duration::from_secs(5)
            );
        }
        for (index, request) in media_requests.iter().enumerate() {
            assert!(request.method == "GET" && request.server_name == MEDIA_HOST);
            assert!(request.target == MEDIA_TARGET && request.body.is_empty());
            assert!(request.header("host") == Some(MEDIA_HOST));
            assert!(request.header("accept") == Some("audio/wav"));
            assert!(request.header("accept-encoding") == Some("identity"));
            assert!(request.received_at >= requests[if index == 0 { 4 } else { 7 }].received_at);
            for name in [
                "authorization",
                "cookie",
                "referer",
                "range",
                "proxy-authorization",
                "x-hito-plugin-protocol",
            ] {
                assert!(request.header(name).is_none());
            }
        }
        assert!(pending.binding() == &binding);
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        let private = [
            TOKEN,
            &binding.info_hash,
            "private-cancellation-album",
            "owned-cancellation.wav",
            &link,
            "owned-private-cancellation-leaf",
            "owned-private-cancellation-signature",
        ];
        polls.assert_workflows(&[("delayed_file_poll", "candidate", 1); 3], &private);
        assert_opening_logs(
            &owner,
            &[
                ("magnet_observation", "cancelled", 0),
                ("bound_audio_opening", "cancelled", 0),
            ],
            &private,
        );
        assert!(
            decoy.is_empty(),
            "the decoy dispatcher must receive no owner completion"
        );
        assert_opening_logs(
            &recovery,
            &[
                ("magnet_observation", "provider_completed", 1),
                ("bound_audio_opening", "complete", 1),
                ("magnet_observation", "provider_completed", 1),
                ("bound_audio_opening", "complete", 1),
                ("body", "complete", WAV_BYTES),
                ("body", "complete", WAV_BYTES),
            ],
            &private,
        );
    })
    .await
    .expect("the bounded real-clock cancellation and recovery workflow exceeded its deadline");
}

fn silent_wave() -> Vec<u8> {
    let data_bytes = WAV_BYTES as u32 - 44;
    let mut bytes = Vec::with_capacity(WAV_BYTES as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(data_bytes + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&192_000_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    bytes.resize(WAV_BYTES as usize, 0);
    bytes
}

fn assert_opening_logs(capture: &Capture, expected: &[(&str, &str, u64)], private: &[&str]) {
    let text = capture.text();
    for value in private.iter().copied().chain([
        "http://",
        "https://",
        "http%3A",
        "https%3A",
        "Bearer ",
        "owned-private-media-cookie",
        "owned-private-upstream-name.wav",
    ]) {
        let escaped = serde_json::to_string(value).unwrap();
        assert!(!text.contains(value) && !text.contains(&escaped[1..escaped.len() - 1]));
    }
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), expected.len());
    let mut correlations = BTreeSet::new();
    let mut openings = BTreeSet::new();
    let mut parents = BTreeSet::new();
    for (entry, (operation, outcome, count)) in events.iter().zip(expected) {
        assert_keys(entry, &["level", "fields", "target", "span", "spans"]);
        assert_eq!(entry["level"], "DEBUG");
        let body = *operation == "body";
        assert_eq!(
            entry["target"],
            if body {
                "hito_remote_audio::body"
            } else {
                "hito_debrid_all_debrid::client"
            }
        );
        let fields = &entry["fields"];
        assert_keys(
            fields,
            if body {
                &["event", "message", "outcome", "bytes", "elapsed_ms"]
            } else {
                &["event", "outcome", "count", "elapsed_ms"]
            },
        );
        let event = match *operation {
            "magnet_observation" => "all_debrid_observation_finished",
            "bound_audio_opening" => "all_debrid_audio_opening_finished",
            "body" => "remote_audio_body_finished",
            _ => unreachable!(),
        };
        assert_eq!(fields["event"], event);
        assert_eq!(fields["outcome"], *outcome);
        assert_eq!(
            fields[if body { "bytes" } else { "count" }].as_u64(),
            Some(*count)
        );
        assert!(fields["elapsed_ms"].as_u64().is_some());
        let spans = entry["spans"].as_array().unwrap();
        assert_eq!(
            spans.len(),
            if *operation == "magnet_observation" {
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
                _ => unreachable!(),
            };
            assert_eq!(span["name"], name);
            assert_eq!(
                uuid::Uuid::parse_str(span["request_id"].as_str().unwrap())
                    .unwrap()
                    .get_version_num(),
                4
            );
        }
        assert_eq!(
            current["operation"],
            if body {
                "bound_audio_opening"
            } else {
                operation
            }
        );
        let correlation = current["request_id"].as_str().unwrap().to_owned();
        if body {
            assert!(openings.contains(&correlation));
            assert_eq!(fields["message"], "Remote audio body finished");
        } else {
            assert!(correlations.insert(correlation.clone()));
            if *operation == "bound_audio_opening" {
                assert!(openings.insert(correlation));
            } else {
                assert_eq!(spans[0]["operation"], "bound_audio_opening");
                assert!(parents.insert(spans[0]["request_id"].as_str().unwrap().to_owned()));
            }
        }
    }
    assert_eq!(parents, openings);
    assert_eq!(correlations.len(), openings.len() * 2);
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys, expected.iter().copied().collect());
}
