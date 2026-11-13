use crate::resolution_musical_fixture as fixture;
use hito_core::test_support::hang_guard::HANG_CEILING;

use futures_util::FutureExt;
use hito_core::{Error, ProviderSearch, ResolveSource, SourceResolutionOutcome};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn held_provider_capacity_is_unavailable_and_release_allows_a_valid_source() {
    let primary = fixture::ProviderServer::start().await;
    let audio = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(HANG_CEILING, AssertUnwindSafe(async {
        let owned = fixture::SavedRecording::prepare(&primary, &audio).await;
        let held = audio.hold("v1/search").await;
        let request_id = Uuid::new_v4();
        let (search, resolution) = tokio::join!(
            owned.hito.catalog.search_provider(
                &owned.actor, &owned.audio_installation.id,
                query(owned.audio_installation.revision),
            ),
            async {
                held.entered().await;
                let rejected = owned.hito.catalog.search_provider(
                    &owned.actor, &owned.audio_installation.id,
                    query(owned.audio_installation.revision),
                ).await;
                assert!(matches!(rejected, Err(Error::ProviderBusy(_))),
                    "ordinary public provider search must identify typed control capacity exhaustion");
                let resolution = owned.hito.catalog.resolve_source_until(
                    &owned.actor, &owned.recording, ResolveSource { request_id },
                    std::future::pending(),
                ).await.unwrap_or_else(|_| panic!("capacity exhaustion must settle a typed receipt"));
                assert!(resolution.request_id == request_id
                    && resolution.recording_id == owned.recording && resolution.sources.is_empty(),
                    "a held control exchange must not publish another recording or source");
                let replay = owned.hito.catalog.resolve_source_until(
                    &owned.actor, &owned.recording, ResolveSource { request_id },
                    std::future::pending(),
                ).await.unwrap_or_else(|_| panic!("the accepted capacity receipt must remain replayable"));
                assert!(serde_json::to_value(&replay).unwrap() == serde_json::to_value(&resolution).unwrap(),
                    "the exact UUID must replay the accepted capacity result without replacing its identity");
                assert!(audio.requests("v1/search").await.len() == 1
                    && audio.requests("v1/recordings/detail").await.is_empty()
                    && audio.requests("v1/sources").await.is_empty()
                    && audio.requests("v1/recordings/metadata").await.is_empty()
                    && audio.requests("v1/audio").await.is_empty(),
                    "capacity rejection and replay must not dispatch any additional provider work");
                owned.assert_preserved().await;
                assert!(matches!(owned.hito.downloads.status(&owned.actor, &owned.recording).await,
                    Err(Error::NotFound)), "capacity rejection must not create a download");
                held.open();
                resolution
            },
        );
        let page = search.unwrap_or_else(|_| panic!("the exact held search must complete after release"));
        assert!(page.items.len() == 1 && page.validate(20).is_ok(),
            "the held owner must receive the real valid provider page");
        held.open();
        let recovery = owned.hito.catalog.resolve_source_until(
            &owned.actor, &owned.recording,
            ResolveSource { request_id: Uuid::new_v4() }, std::future::pending(),
        ).await.unwrap_or_else(|_| panic!("a fresh request must resolve after control capacity returns"));
        assert!(matches!(recovery.outcome, SourceResolutionOutcome::Ready)
            && recovery.resolution_id != resolution.resolution_id
            && recovery.recording_id == owned.recording
            && recovery.sources.len() == 1 && recovery.sources[0].ready,
            "released capacity must allow a new receipt to publish the valid source on the same recording");
        assert!(audio.requests("v1/search").await.len() == 2
            && audio.requests("v1/recordings/detail").await.len() == 1
            && audio.requests("v1/sources").await.len() == 1
            && audio.requests("v1/recordings/metadata").await.len() == 1
            && audio.requests("v1/audio").await.is_empty(),
            "only the fresh recovery may perform candidate hydration, without fetching audio");
        owned.assert_preserved().await;
        assert_eq!(serde_json::to_value(resolution).unwrap()["outcome"], json!("provider_unavailable"),
            "held provider capacity must not be recorded as a completed search without suitable audio");
    }).catch_unwind()).await;
    primary.shutdown().await;
    audio.shutdown().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned capacity workflow must settle within its deadline"),
    }
}

fn query(installation_revision: i64) -> ProviderSearch {
    ProviderSearch {
        installation_revision,
        query: "Owned performer Owned tune".into(),
        cursor: None,
        limit: 20,
    }
}
