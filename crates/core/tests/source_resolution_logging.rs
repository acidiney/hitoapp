#[path = "fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod fixture;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{ResolveSource, SourceResolutionOutcome};
use serde_json::{Value, json};
use std::{
    io::Write,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn rejected_musical_candidate_logs_safe_search_counts_and_missing_title_reason() {
    let spotify = fixture::ProviderServer::start().await;
    let youtube = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        AssertUnwindSafe(async {
            let owned = fixture::SavedRecording::prepare_named_target(
                &spotify,
                &youtube,
                "private-target-title-log-canary",
                "private-target-album-log-canary",
            )
            .await;
            let recording = json!({
                "id": "private-provider-id-log-canary",
                "title": "private-candidate-title-log-canary",
                "artist": "Channel: private-candidate-artist-log-canary",
                "duration_ms": 1000,
                "revision": "https://provider.invalid/private-log-canary?signature=private-secret-log-canary"
            });
            youtube.reply("v1/search", json!({"items": [recording], "next_cursor": null})).await;
            youtube.reply("v1/recordings/detail", recording.clone()).await;
            youtube.reply("v1/recordings/metadata", json!({
                "recording": recording,
                "musical_title": null,
                "album_title": null,
                "source_ids": ["private-source-id-log-canary"]
            })).await;
            youtube.reply("v1/sources", json!({"items": [{
                "id": "private-source-id-log-canary",
                "label": "private-audio-label-log-canary", "ready": true
            }]})).await;
            let before = owned.hito.catalog.recording_metadata(
                &owned.actor, &owned.recording, Default::default(),
            ).await.unwrap();
            let logs = CapturedLogs::default();
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter("off,hito_core=debug")
                .with_ansi(false)
                .without_time()
                .json()
                .with_writer(logs.clone())
                .finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            let request_id = Uuid::new_v4();
            let resolution = owned.hito.catalog.resolve_source_until(
                &owned.actor,
                &owned.recording,
                ResolveSource { request_id },
                std::future::pending(),
            ).await.unwrap();
            assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
            assert!(resolution.sources.len() == 1);
            owned.assert_preserved().await;
            let after = owned.hito.catalog.recording_metadata(
                &owned.actor, &owned.recording, Default::default(),
            ).await.unwrap();
            assert!(fixture::encoded(&before.selected_observation) == fixture::encoded(&after.selected_observation));
            assert!(youtube.requests("v1/recordings/metadata").await.len() == 1);
            assert!(youtube.requests("v1/audio").await.is_empty());
            let captured = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
            for private in [
                "private-target-title-log-canary", "private-target-album-log-canary",
                "private-provider-id-log-canary", "private-candidate-title-log-canary",
                "private-candidate-artist-log-canary", "private-source-id-log-canary",
                "private-audio-label-log-canary", "private-secret-log-canary",
                "https://provider.invalid/private-log-canary", "Owned performer",
                &spotify.url, &youtube.url,
            ] {
                assert!(!captured.contains(private), "source resolution logs must exclude private protocol values");
            }
            let events = captured.lines().map(|line| {
                serde_json::from_str::<Value>(line).expect("captured logs must use structured JSON")
            }).collect::<Vec<_>>();
            let search = events.iter().find(|event| {
                event["fields"]["event"] == "source_resolution_search_candidates"
            }).expect("source resolution must log safe search candidate counts");
            assert!(search["level"] == "DEBUG");
            assert!(search["fields"]["returned_count"] == 1);
            assert!(search["fields"]["plausible_count"] == 1);
            assert!(search["fields"]["detail_limit"] == 3);
            let rejected = events.iter().find(|event| {
                event["fields"]["event"] == "source_resolution_candidate_rejected"
            }).expect("source resolution must log a static candidate rejection reason");
            assert!(rejected["level"] == "DEBUG");
            assert!(rejected["fields"]["reason"] == "missing_musical_title");
            let fallback = events.iter().find(|event| {
                event["fields"]["event"] == "source_resolution_provider_rank_fallback"
            }).expect("the provider-rank fallback must be logged");
            assert!(fallback["level"] == "INFO");
            assert!(fallback["fields"]["outcome"] == "ready");
            assert!(fallback["fields"]["candidate_rank"] == 0);
            assert!(fallback["fields"]["examined"] == 1);
            assert!(fallback["fields"]["elapsed_ms"].is_u64());
            for event in [search, rejected, fallback] {
                assert!(event["span"]["request_id"] == request_id.to_string(),
                    "candidate diagnostics must retain the source resolution correlation identity");
            }
        }).catch_unwind(),
    ).await;
    spotify.shutdown().await;
    youtube.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the source resolution log exercise must settle within its deadline"),
    }
}
