#![cfg(feature = "test-support")]

#[path = "qualification/mod.rs"]
pub mod qualification;

use qualification::{
    headers, provider::*, run_media, transfer, wave_fixture, wire_fixture::Response,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[tokio::test]
async fn dropping_only_the_http_body_closes_the_media_peer_and_a_fresh_request_still_plays() {
    let wave = wave_fixture::wav();
    let (held, witness) = Response::wave(wave.clone()).held(Some(44));
    run_media(
        vec![transfer("finished", 1.0), item(LINK_A), transfer("finished", 1.0), item(LINK_B)],
        vec![held, Response::wave(wave.clone())],
        |case| Box::pin(async move {
            case.release_after_outcome(witness.release);
            let config = case.config();
            let survivor = config.connections[0].client.clone();
            let unrelated_caller = caller();
            let caller = caller();
            let base = case.start(config).await;
            let target = base.join(&format!("v1/audio?source_id={SOURCE}")).unwrap();
            let mut first = caller.get(target.clone()).headers(headers(Some(SERVICE)))
                .send().await.expect("the held public audio request must open");
            let cancelled = case.record_response(&first, StatusCode::OK);
            assert_eq!(first.headers()["content-type"], "audio/wav");
            assert_eq!(first.content_length(), Some(wave_fixture::WAV_BYTES as u64));
            let prefix = first.chunk().await.expect("the held HTTP prefix must arrive")
                .expect("the held HTTP body must contain its prefix");
            assert_eq!(prefix.as_ref(), &wave[..44]);
            tokio::time::timeout(Duration::from_secs(2), witness.reached).await
                .expect("the provider gate must be reached").expect("the provider gate must signal");
            assert_eq!(case.counts(), [2, 1, 0, 0]);

            let started = Instant::now();
            drop(first);
            tokio::time::timeout(Duration::from_secs(2), witness.peer_closed).await
                .expect("dropping the HTTP body must close the actual media TLS peer")
                .expect("the actual TLS peer closure must be witnessed");
            let peer_eof_elapsed = started.elapsed();
            assert!(peer_eof_elapsed <= Duration::from_secs(2));
            assert_eq!(case.counts(), [2, 1, 0, 0]);

            let mut second = caller.get(target).headers(headers(Some(SERVICE)))
                .send().await.expect("fresh audio must open after cancellation");
            let complete = case.record_response(&second, StatusCode::OK);
            assert_eq!(second.headers()["content-type"], "audio/wav");
            assert_eq!(second.content_length(), Some(wave_fixture::WAV_BYTES as u64));
            for name in ["set-cookie", "content-disposition", "location", "authorization", "content-encoding"] {
                assert!(second.headers().get(name).is_none());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = second.chunk().await.expect("fresh WAV chunks must complete") {
                assert!(!chunk.is_empty());
                assert!(chunk.len() <= wave_fixture::WAV_BYTES.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk);
            }
            assert!(second.chunk().await.expect("fresh WAV EOF must complete").is_none());
            assert!(bytes == wave, "the fresh HTTP relay changed the exact owned WAV");
            wave_fixture::assert_decoded(bytes);
            assert_eq!(case.counts(), [4, 2, 0, 0]);
            audio_logs(case.events(), cancelled, complete);
            drop(survivor);
            drop(unrelated_caller);
            drop(caller);
            println!("{}", json!({"event":"owned_http_cancellation_verified",
                "peer_eof_elapsed_ms":peer_eof_elapsed.as_millis(), "cancelled_bytes":44,
                "complete_bytes":wave_fixture::WAV_BYTES, "provider_api_reads":4, "media_requests":2}));
        }),
    ).await;
}

fn caller() -> Client {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap()
}

fn item(link: &str) -> Response {
    Response::json(serde_json::to_vec(&json!({"status":"success", "id":ITEM, "name":FILE,
        "size":wave_fixture::WAV_BYTES, "created_at":ITEM_CREATED, "folder_id":FOLDER, "link":link})).unwrap())
}

fn audio_logs(events: Vec<Value>, cancelled: Uuid, complete: Uuid) {
    let mut counts = BTreeMap::new();
    let mut openings = BTreeMap::new();
    let mut bodies = BTreeMap::new();
    let mut completed = BTreeSet::new();
    let mut audio = BTreeSet::new();
    for event in events {
        keys(&event, &["level", "fields", "target", "span", "spans"]);
        assert_eq!(event["level"], "DEBUG");
        let spans = event["spans"].as_array().unwrap();
        assert_eq!(&event["span"], spans.last().unwrap());
        for span in spans {
            keys(span, &["name", "request_id", "operation"]);
            let id = Uuid::parse_str(span["request_id"].as_str().unwrap()).unwrap();
            assert_eq!(id.get_version_num(), 4);
            assert!(
                (span["name"] == "premiumize_plugin_request" && span["operation"] == "audio")
                    || (span["name"] == "premiumize_audio"
                        && span["operation"] == "bound_audio_opening")
            );
        }
        let owner = spans
            .iter()
            .find(|span| span["name"] == "premiumize_plugin_request")
            .unwrap();
        let owner = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
        assert!(owner == cancelled || owner == complete);
        let fields = &event["fields"];
        assert!(fields["elapsed_ms"].as_u64().is_some());
        let name = fields["event"].as_str().unwrap();
        *counts.entry(name.to_owned()).or_insert(0_usize) += 1;
        match name {
            "premiumize_plugin_request_finished" => {
                keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                assert_eq!(event["target"], "hito_plugin_premiumize::http");
                assert_eq!(fields["outcome"], "success");
                assert_eq!(fields["status"], 200);
                assert!(completed.insert(owner));
            }
            "premiumize_audio_opening_finished" => {
                keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                assert_eq!(event["target"], "hito_debrid_premiumize::audio");
                assert_eq!(fields["outcome"], "complete");
                assert_eq!(fields["count"], 1);
                assert!(
                    openings
                        .insert(owner, event["span"]["request_id"].clone())
                        .is_none()
                );
            }
            "remote_audio_body_finished" | "premiumize_plugin_audio_finished" => {
                let (outcome, bytes) = if owner == cancelled {
                    ("cancelled", 44)
                } else {
                    ("complete", wave_fixture::WAV_BYTES as u64)
                };
                assert_eq!(fields["outcome"], outcome);
                assert_eq!(fields["bytes"].as_u64(), Some(bytes));
                if name == "remote_audio_body_finished" {
                    keys(
                        fields,
                        &["event", "message", "outcome", "bytes", "elapsed_ms"],
                    );
                    assert_eq!(fields["message"], "Remote audio body finished");
                    assert_eq!(event["target"], "hito_remote_audio::body");
                    assert!(
                        bodies
                            .insert(owner, event["span"]["request_id"].clone())
                            .is_none()
                    );
                } else {
                    keys(fields, &["event", "outcome", "bytes", "elapsed_ms"]);
                    assert_eq!(event["target"], "hito_plugin_premiumize::audio");
                    assert!(audio.insert(owner));
                }
            }
            _ => panic!("the two audio requests emitted an unexpected workflow event"),
        }
    }
    let owners = BTreeSet::from([cancelled, complete]);
    assert_eq!(completed, owners);
    assert_eq!(audio, owners);
    assert_eq!(openings, bodies);
    assert_eq!(openings.keys().copied().collect::<BTreeSet<_>>(), owners);
    assert_eq!(
        counts,
        BTreeMap::from([
            ("premiumize_plugin_request_finished".into(), 2),
            ("premiumize_audio_opening_finished".into(), 2),
            ("remote_audio_body_finished".into(), 2),
            ("premiumize_plugin_audio_finished".into(), 2),
        ])
    );
}

fn keys(value: &Value, expected: &[&str]) {
    let actual: BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual, expected.iter().copied().collect());
}
