#![cfg(all(feature = "test-support", unix))]

#[allow(dead_code)]
#[path = "support/restart_catalog_entries.rs"]
mod entries;
#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
mod fixtures;
#[allow(dead_code)]
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
mod http;
#[allow(dead_code)]
#[path = "support/logs.rs"]
mod logs;
#[allow(dead_code)]
#[path = "../../hito-debrid-torbox/tests/audio/media_fixture.rs"]
mod media_fixture;
#[allow(dead_code)]
#[path = "../../hito-debrid-torbox/tests/support/mod.rs"]
mod provider;
#[allow(dead_code)]
#[path = "bootstrap_support/snapshot.rs"]
mod snapshot;
#[allow(dead_code)]
#[path = "support/state_inputs.rs"]
mod state_inputs;

use futures_util::{FutureExt, future};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{ProviderRecording, RecordingAttribution, RecordingRequest};
use hito_plugin_torbox::{Config, CuratedWavRecording, InitError, Service, StateConfig, StateMode};
use reqwest::StatusCode;
use serde_json::Value;
use snapshot::Snapshot;
use std::{collections::BTreeSet, fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt,
          panic::AssertUnwindSafe, path::Path, time::Duration};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const SERVICE_TOKEN: &str = "owned-private-torbox-detail-bearer";
const WRONG_TOKEN: &str = "owned-private-torbox-detail-wrong-bearer";
const COOKIE: &str = "owned-private-torbox-detail-cookie";
const TITLE: &str = "owned-private-catalog-title";
const ARTIST: &str = "owned-private-catalog-artist";
const QUERY: &str = "owned-private-torbox-detail-query";

struct Attempt {
    outcome: &'static str,
    active_preserved: bool,
    closed_preserved: bool,
    joined: bool,
}

#[tokio::test]
async fn detail_budget_rejection_precedes_private_state_ownership_and_provider_work() {
    let provider = provider::Fixture::start(vec![]).await;
    let media = media_fixture::MediaFixture::start(fixtures::wave()).await;
    let inputs = state_inputs::Inputs::new();
    let directory = inputs.directory.path();
    let key_path = directory.join("candidate.key");
    let original_path = directory.join("owned-original.wav");
    let mut original = OpenOptions::new().write(true).create_new(true).mode(0o600)
        .open(&original_path).unwrap();
    original.write_all(&fixtures::wave()).unwrap();
    original.sync_all().unwrap();
    drop(original);
    let key_before = Snapshot::capture(&key_path);
    let original_before = Snapshot::capture(&original_path);
    let capture = logs::Capture::default();
    let dispatch = capture.dispatch();
    let mut owner = None;
    let mut server = None;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(20), async {
        let boundary = recording_bytes(32_768);
        let expected = boundary.recording.clone();
        owner = Some(Service::open(config(&provider, &media, inputs.next(), boundary))
            .with_subscriber(dispatch.clone()).await.expect("the exact-boundary catalog must initialize"));
        server = Some(http::Server::start(owner.as_ref().unwrap().router(), dispatch.clone()).await);
        exact_detail(server.as_ref().unwrap(), &expected).await;
        let http_joined = server.take().unwrap().close().await.is_ok();
        let state_joined = owner.take().unwrap().close().await.is_ok();
        assert!(http_joined && state_joined, "the valid owner must join HTTP before private state");
        let existing_before = Snapshot::capture(directory);
        let existing = attempt(config(&provider, &media, inputs.next(), recording_bytes(32_769)),
            &mut owner, directory, &existing_before, &dispatch).await;
        let absent_before = Snapshot::capture(directory);
        let mut state = inputs.next();
        state.directory = directory.join("absent-journal");
        state.mode = StateMode::CreateNew;
        let absent = attempt(config(&provider, &media, state, recording_bytes(32_769)),
            &mut owner, directory, &absent_before, &dispatch).await;
        (existing, absent, directory.join("absent-journal").exists())
    })).catch_unwind().await;
    let http_joined = match server.take() { Some(server) => server.close().await.is_ok(), None => true };
    let state_joined = match owner.take() { Some(owner) => owner.close().await.is_ok(), None => true };
    let (requests, media_requests) = future::join(provider.close(), media.close()).await;
    assert!(http_joined && state_joined, "cleanup must join HTTP before private state");
    let (existing, absent, created) = match outcome {
        Ok(result) => result.expect("the public detail-budget workflow exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(requests.is_empty() && media_requests.is_empty(), "metadata must not contact provider or media");
    assert!(existing.joined && absent.joined, "every unexpectedly accepted owner must be joined");
    assert!(key_before.unchanged(&Snapshot::capture(&key_path))
        && original_before.unchanged(&Snapshot::capture(&original_path)), "operator key and original must remain exact");
    let owners = 1 + usize::from(existing.outcome == "opened") + usize::from(absent.outcome == "opened");
    assert_logs(&capture.snapshot(), owners, directory);
    eprintln!("detail_budget_evidence served_bytes=32768 overlimit_bytes=32769 existing_outcome={} absent_outcome={} existing_active_preserved={} existing_closed_preserved={} absent_active_preserved={} absent_closed_preserved={} absent_created={created} provider_requests=0 media_requests=0 joined=true",
        existing.outcome, absent.outcome, existing.active_preserved, existing.closed_preserved,
        absent.active_preserved, absent.closed_preserved);
    assert!(existing.outcome == "invalid_catalog" && absent.outcome == "invalid_catalog"
        && existing.active_preserved && existing.closed_preserved
        && absent.active_preserved && absent.closed_preserved && !created,
        "SDK-valid 32769-byte metadata must return InvalidCatalog before creating or touching private state");
}

async fn attempt(config: Config, active: &mut Option<Service>, directory: &Path,
                 before: &Snapshot, dispatch: &tracing::Dispatch) -> Attempt {
    let outcome = match Service::open(config).with_subscriber(dispatch.clone()).await {
        Err(InitError::InvalidCatalog) => "invalid_catalog",
        Err(_) => "other_rejection",
        Ok(owner) => { *active = Some(owner); "opened" }
    };
    let active_preserved = before.unchanged(&Snapshot::capture(directory));
    let joined = match active.take() { Some(owner) => owner.close().await.is_ok(), None => true };
    let closed_preserved = before.unchanged(&Snapshot::capture(directory));
    Attempt { outcome, active_preserved, closed_preserved, joined }
}

fn config(provider: &provider::Fixture, media: &media_fixture::MediaFixture,
          state: StateConfig, recording: CuratedWavRecording) -> Config {
    Config {
        state,
        service_bearer: SERVICE_TOKEN.into(),
        client: TorBoxClient::with_test_tls_media(fixtures::TOKEN.into(), provider.ca_der(),
            provider.address(), media.ca_der(), media.address()).unwrap(),
        media_origins: MediaAllowlist::new(vec![format!("https://{}", fixtures::MEDIA_HOST)]).unwrap(),
        recordings: vec![recording],
    }
}

async fn exact_detail(server: &http::Server, expected: &ProviderRecording) {
    let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never()).timeout(Duration::from_secs(4)).build().unwrap();
    let mut response = client.post(server.base.join("v1/recordings/detail").unwrap())
        .bearer_auth(SERVICE_TOKEN).header("x-hito-plugin-protocol", "1").header("cookie", COOKIE)
        .json(&RecordingRequest { recording_id: entries::RECORDING.into() }).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    for name in ["location", "set-cookie", "content-disposition", "etag"] {
        assert!(response.headers().get(name).is_none());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(chunk.len() <= 32_768_usize.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes.len(), 32_768);
    assert!(bytes == serde_json::to_vec(expected).unwrap(), "the complete typed metadata bytes changed");
    let actual: ProviderRecording = serde_json::from_slice(&bytes).unwrap();
    actual.validate().unwrap();
    assert!(actual == *expected);
    let text = std::str::from_utf8(&bytes).unwrap();
    for private in [fixtures::TOKEN, SERVICE_TOKEN, WRONG_TOKEN, COOKIE,
        fixtures::MEDIA_TARGET, "0123456789abcdef0123456789abcdef01234567", "album/owned.wav"] {
        assert!(!text.contains(private), "metadata leaked private provider or caller material");
    }
}

fn recording_bytes(bytes: usize) -> CuratedWavRecording {
    let mut entry = entries::original(false).pop().unwrap();
    entry.recording.attribution = Some(RecordingAttribution {
        creators: vec!["a".repeat(2048); 16],
        provider_label: "owned-private-detail-budget-attribution".into(),
        content_url: "https://example.invalid/owned-public-attribution".into(),
    });
    let encoded = serde_json::to_vec(&entry.recording).unwrap().len();
    assert!(encoded > bytes && encoded - bytes < 2048);
    entry.recording.attribution.as_mut().unwrap().creators[15].truncate(2048 - (encoded - bytes));
    entry.recording.validate().unwrap();
    assert_eq!(serde_json::to_vec(&entry.recording).unwrap().len(), bytes);
    entry
}

fn assert_logs(bytes: &[u8], owners: usize, directory: &Path) {
    let text = std::str::from_utf8(bytes).unwrap();
    for private in [fixtures::TOKEN, SERVICE_TOKEN, WRONG_TOKEN, COOKIE, TITLE, ARTIST, QUERY,
        entries::RECORDING, entries::SOURCE, "album/owned.wav", fixtures::MEDIA_TARGET,
        "0123456789abcdef0123456789abcdef01234567", "http://", "https://", "Bearer ", "token=",
        directory.to_str().unwrap()] {
        let escaped = serde_json::to_string(private).unwrap();
        assert!(!text.contains(private) && !text.contains(&escaped[1..escaped.len()-1]), "logs exposed private data");
    }
    let events: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let mut counts = [0; 4];
    let mut correlations = BTreeSet::new();
    for event in &events {
        assert_eq!(event["level"], "DEBUG");
        let root = event.as_object().unwrap();
        assert!(root.len() == 5 && root.keys().all(|k| matches!(k.as_str(), "level"|"fields"|"target"|"span"|"spans")));
        let fields = event["fields"].as_object().unwrap();
        assert!(fields.len() == 4 && event["fields"]["elapsed_ms"].as_u64().is_some_and(|v| v <= 30_000));
        let spans = event["spans"].as_array().unwrap();
        assert!(!spans.is_empty() && Some(&event["span"]) == spans.last());
        for span in spans {
            assert!(span.as_object().unwrap().len() == 3 && span.as_object().unwrap().keys()
                .all(|k| matches!(k.as_str(), "name"|"request_id"|"operation")));
            assert_eq!(Uuid::parse_str(span["request_id"].as_str().unwrap()).unwrap().get_version_num(), 4);
        }
        assert!(correlations.insert(event["span"]["request_id"].as_str().unwrap()));
        let (index, target, span, operation, outcome) = match event["fields"]["event"].as_str().unwrap() {
            "torbox_candidate_authority_finished" => (0, "hito_debrid_torbox::workflow", "torbox_candidate_persistence", "candidate_authority", "complete"),
            "torbox_journal_open_finished" => (1, "hito_plugin_torbox::journal", "torbox_journal", "journal_open", "ready"),
            "torbox_journal_close_finished" => (2, "hito_plugin_torbox::journal", "torbox_journal", "journal_close", "joined"),
            "torbox_plugin_request_finished" => (3, "hito_plugin_torbox::policy", "torbox_plugin_request", "detail", "success"),
            _ => panic!("metadata admission emitted an unexpected workflow class"),
        };
        assert_eq!(event["target"], target);
        assert_eq!(event["span"]["name"], span);
        assert_eq!(event["span"]["operation"], operation);
        assert_eq!(event["fields"]["outcome"], outcome);
        let numeric = if index == 3 { "status" } else { "count" };
        assert!(fields.keys().all(|k| matches!(k.as_str(), "event"|"outcome"|"elapsed_ms") || k == numeric));
        assert_eq!(event["fields"][numeric].as_u64(), Some(if index == 3 { 200 } else if index == 0 { 1 } else { 0 }));
        counts[index] += 1;
    }
    assert_eq!(counts, [owners, owners, owners, 1]);
}
