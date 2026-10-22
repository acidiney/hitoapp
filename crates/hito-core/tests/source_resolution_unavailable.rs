#[path = "fixtures/resolution_unavailable.rs"]
mod fixture;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::ResolveSource;
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn provider_source_failure_is_not_committed_as_an_unsuitable_recording() {
    let primary = fixture::ProviderServer::start().await;
    let audio = fixture::ProviderServer::start().await;
    let exercise = tokio::time::timeout(Duration::from_secs(12), AssertUnwindSafe(async {
        let owned = fixture::SavedLibrary::prepare(&primary, &audio).await;
        let detail = audio.hold("v1/recordings/detail").await;
        let request_id = Uuid::new_v4();
        let (resolution, ()) = tokio::join!(
            owned.hito.catalog.resolve_source_until(
                &owned.actor, &owned.recording, ResolveSource { request_id },
                std::future::pending(),
            ),
            async {
                detail.entered().await;
                audio.offline().await;
                detail.open();
            },
        );
        let resolution = resolution
            .unwrap_or_else(|_| panic!("the provider failure must return a settled typed resolution"));
        assert!(resolution.request_id == request_id && resolution.recording_id == owned.recording
            && resolution.sources.is_empty(),
            "the failed exchange must retain request and recording identities without sources");
        let searches = audio.requests("v1/search").await;
        let details = audio.requests("v1/recordings/detail").await;
        let sources = audio.requests("v1/sources").await;
        assert!(searches.len() == 1 && details.len() == 1 && sources.len() == 1
            && searches[0].1 == json!({"query": "Owned performer Owned tune", "cursor": null, "limit": 20})
            && details[0].1 == json!({"recording_id": "audio/owned-recording"})
            && sources[0].1 == details[0].1,
            "one plausible search and successful detail must reach the real failing source exchange");
        assert!(audio.requests("v1/recordings/metadata").await.is_empty()
            && audio.requests("v1/audio").await.is_empty(),
            "a source failure must not request metadata witnesses or audio");
        owned.assert_preserved().await;
        let replay = owned.hito.catalog.resolve_source_until(
            &owned.actor, &owned.recording, ResolveSource { request_id },
            std::future::pending(),
        ).await.unwrap_or_else(|_| panic!("the accepted failure receipt must remain replayable"));
        let serialized: Value = serde_json::to_value(&resolution)
            .unwrap_or_else(|_| panic!("the public resolution must serialize"));
        let replayed = serde_json::to_value(replay)
            .unwrap_or_else(|_| panic!("the public receipt replay must serialize"));
        assert!(replayed == serialized,
            "the same UUID must replay the exact accepted failure receipt");
        assert!(audio.requests("v1/search").await.len() == 1
            && audio.requests("v1/recordings/detail").await.len() == 1
            && audio.requests("v1/sources").await.len() == 1
            && audio.requests("v1/audio").await.is_empty(),
            "accepted failure receipt replay must perform no further provider work");
        owned.assert_preserved().await;
        let response = reqwest::Client::new()
            .post(format!("{}v1/sources", audio.url))
            .json(&hito_plugin_sdk::RecordingRequest {
                recording_id: "audio/owned-recording".into(),
            })
            .send().await
            .unwrap_or_else(|_| panic!("the owned provider failure must remain observable over HTTP"));
        assert!(response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "the exhausted provider must return an actual HTTP503 for its source endpoint");
        for value in ["primary/owned-recording", "audio/owned-recording", "audio/owned-source", "Private title override", &primary.url, &audio.url] {
            assert!(!serialized.to_string().contains(value),
                "the public failure receipt must omit private metadata and provider identifiers");
        }
        assert_eq!(serialized["outcome"], json!("provider_unavailable"),
            "a real provider source503 must remain distinct from a completed search without suitable audio");
    }).catch_unwind()).await;
    primary.shutdown().await;
    audio.shutdown().await;
    match exercise {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the provider-unavailable regression must settle within its owned deadline")
        }
    }
}
