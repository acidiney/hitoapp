use hito_core::test_support::hang_guard::HANG_CEILING;
#[path = "../fixtures/resolution_failover.rs"]
mod protocol;
#[path = "../fixtures/resolution_musical.rs"]
#[allow(dead_code)]
mod saved;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{Error, OpenedAudio, ResolveSource};
use protocol::{Event, Plan};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn successful_empty_search_stays_distinct_from_provider_search_failure() {
    exercise(Plan::Empty, "no_suitable_source").await;
    exercise(Plan::SearchUnavailable, "provider_unavailable").await;
}

#[tokio::test]
async fn failed_source_candidate_does_not_hide_the_next_valid_candidate() {
    exercise(Plan::FailedThenReady, "ready").await;
}

async fn exercise(plan: Plan, outcome: &str) {
    let primary = saved::ProviderServer::start().await;
    let previous_audio = saved::ProviderServer::start().await;
    let mut provider = None;
    let result = tokio::time::timeout(HANG_CEILING, AssertUnwindSafe(async {
        let owned = saved::SavedRecording::prepare(&primary, &previous_audio).await;
        owned.hito.plugins.remove(&owned.actor, &owned.audio_installation.id, owned.audio_installation.revision).await
            .unwrap_or_else(|_| panic!("the unused setup connection must be removed before the protocol exercise"));
        provider = Some(protocol::Provider::start(plan, owned.original.clone()).await);
        let provider = provider.as_ref().unwrap();
        provider.connect(&owned.hito, &owned.actor).await;
        let request_id = Uuid::new_v4();
        let resolution = owned.hito.catalog.resolve_source_until(
            &owned.actor, &owned.recording, ResolveSource { request_id }, std::future::pending(),
        ).await.unwrap_or_else(|_| panic!("the owned protocol outcome must settle a typed receipt"));
        assert!(resolution.request_id == request_id && resolution.recording_id == owned.recording,
            "the protocol outcome must retain the request and imported recording identities");
        assert_eq!(serde_json::to_value(&resolution).unwrap()["outcome"], json!(outcome),
            "successful exhaustion, upstream failure and valid failover must remain distinct");
        let events = provider.events().await;
        let expected = match plan {
            Plan::Empty | Plan::SearchUnavailable => vec![Event::Manifest, Event::Search],
            Plan::FailedThenReady => vec![Event::Manifest, Event::Search, Event::FailedDetail, Event::FailedSources, Event::ReadyDetail, Event::ReadySources, Event::Metadata],
        };
        assert!(events == expected,
            "the real protocol boundary must observe exact bounded candidate order and no premature audio");
        let replay = owned.hito.catalog.resolve_source_until(
            &owned.actor, &owned.recording, ResolveSource { request_id }, std::future::pending(),
        ).await.unwrap_or_else(|_| panic!("the exact accepted protocol receipt must replay"));
        assert!(serde_json::to_value(replay).unwrap() == serde_json::to_value(&resolution).unwrap()
            && provider.events().await == events,
            "receipt replay must retain the exact result without repeating provider work");
        owned.assert_preserved().await;
        if matches!(plan, Plan::FailedThenReady) {
            assert!(resolution.sources.len() == 1 && resolution.sources[0].ready
                && resolution.sources[0].recording_id == owned.recording,
                "only the valid second candidate may supply one playable source");
            let metadata = owned.hito.catalog.recording_metadata(&owned.actor, &owned.recording, Default::default()).await.unwrap();
            assert!(metadata.observations.items.iter().all(|observation| observation.provider_recording_id.as_deref() != Some(protocol::FAILED))
                && metadata.observations.items.iter().filter(|observation| observation.provider_recording_id.as_deref() == Some(protocol::READY)).count() == 1,
                "the failed first candidate must not publish metadata while the valid second retains its evidence");
            let OpenedAudio::Content(mut audio) = owned.hito.playback.open_audio(
                &owned.actor, &resolution.sources[0].id, Default::default(),
            ).await.unwrap_or_else(|_| panic!("the valid failover source must open through public playback"))
            else { panic!("the valid failover source must supply audio content") };
            let body = audio.body.as_mut().unwrap();
            let mut bytes = Vec::new();
            while let Some(chunk) = body.chunk().await.unwrap() { bytes.extend_from_slice(&chunk); }
            assert!(bytes == owned.original, "the valid failover must serve the exact owned WAV");
            let mut after_audio = expected; after_audio.push(Event::Audio);
            assert!(provider.events().await == after_audio,
                "only explicit public playback may request the selected provider audio");
        } else {
            assert!(resolution.sources.is_empty(), "empty and failed searches must publish no source");
        }
        owned.assert_preserved().await;
        assert!(matches!(owned.hito.downloads.status(&owned.actor, &owned.recording).await, Err(Error::NotFound)),
            "protocol resolution and explicit relay playback must not create persistent downloads");
        for path in ["v1/search", "v1/recordings/detail", "v1/sources", "v1/audio"] {
            assert!(previous_audio.requests(path).await.is_empty(),
                "a removed setup connection must never participate in resolution or playback");
        }
    }).catch_unwind()).await;
    let stopped = if let Some(provider) = provider.as_mut() {
        provider.finish().await
    } else {
        true
    };
    primary.shutdown().await;
    previous_audio.shutdown().await;
    assert!(stopped, "the owned protocol provider must stop and join");
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the bounded source failover protocol exercise must settle"),
    }
}
