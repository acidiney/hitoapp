use hito_core::test_support::community_ranking_library as library;
use hito_core::test_support::community_rankings as remote;
use hito_core::test_support::content_service as provider;

use futures_util::FutureExt;
use hito_core::{
    Actor, OpenedAudio, Services, Source, UpdateCommunityPreferences,
    UpdateCommunityRankingPreferences,
};
use serde_json::json;
use std::{
    future::pending,
    panic::AssertUnwindSafe,
    time::{Duration, Instant},
};
use tokio::{task::JoinSet, time::timeout};

#[tokio::test]
async fn rankings_opt_in_reorders_verified_sources_without_enabling_reports() {
    let primary = provider::ProviderServer::start().await;
    let candidate = provider::ProviderServer::start().await;
    let ranking = remote::RankingServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(&primary, &candidate, &ranking)).catch_unwind(),
    )
    .await;
    primary.shutdown().await;
    candidate.shutdown().await;
    ranking.close().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("The owned source ranking workflow must finish within its deadline"),
    }
}

async fn exercise(
    primary: &provider::ProviderServer,
    candidate: &provider::ProviderServer,
    ranking: &remote::RankingServer,
) {
    let owned = library::Library::prepare(primary, candidate, &ranking.url).await;
    let (hito, actor, recording, baseline) =
        (&owned.hito, &owned.actor, &owned.recording, &owned.baseline);
    let before = owned.private_state().await;
    let disabled = hito.community.ranking_preferences(actor).await.unwrap();
    assert!(disabled.revision == 0 && !disabled.use_source_rankings);
    let legacy = hito.community.preferences(actor).await.unwrap();
    assert!(legacy.revision == 0 && !legacy.playback_reporting && !legacy.metadata_corrections);
    assert_default_order(hito, actor, &recording.id, baseline).await;
    ranking.assert_no_requests().await;
    let reporting = hito
        .community
        .update_preferences(
            actor,
            UpdateCommunityPreferences {
                revision: legacy.revision,
                playback_reporting: true,
                metadata_corrections: false,
            },
        )
        .await
        .unwrap();
    assert_default_order(hito, actor, &recording.id, baseline).await;
    ranking.assert_no_requests().await;
    let corrections = hito
        .community
        .update_preferences(
            actor,
            UpdateCommunityPreferences {
                revision: reporting.revision,
                playback_reporting: false,
                metadata_corrections: true,
            },
        )
        .await
        .unwrap();
    assert_default_order(hito, actor, &recording.id, baseline).await;
    ranking.assert_no_requests().await;
    let legacy = hito
        .community
        .update_preferences(
            actor,
            UpdateCommunityPreferences {
                revision: corrections.revision,
                playback_reporting: false,
                metadata_corrections: false,
            },
        )
        .await
        .unwrap();
    let enabled = hito
        .community
        .update_ranking_preferences(
            actor,
            UpdateCommunityRankingPreferences {
                revision: disabled.revision,
                use_source_rankings: true,
            },
        )
        .await
        .unwrap();
    assert!(enabled.revision == 1 && enabled.use_source_rankings);
    assert_eq!(hito.community.preferences(actor).await.unwrap(), legacy);
    ranking.assert_no_requests().await;
    let expected: Vec<_> = baseline.iter().map(library::public_reference).collect();
    ranking
        .reply(json!({"ranked": [
            {"source": expected[1], "successful_reporters": 9},
            {"source": expected[0], "successful_reporters": 1},
        ]}))
        .await;
    let ranked = hito
        .catalog
        .playback_sources_until(actor, &recording.id, pending())
        .await
        .unwrap();
    assert!(
        serde_json::to_value(&ranked).unwrap()
            == serde_json::to_value(baseline.iter().rev().collect::<Vec<_>>()).unwrap(),
        "Community rankings must change only the order of the exact eligible sources"
    );
    ranking
        .assert_one_public_request(json!({"candidates": expected}))
        .await;
    assert_eq!(hito.community.preferences(actor).await.unwrap(), legacy);
    assert_eq!(
        hito.community.ranking_preferences(actor).await.unwrap(),
        enabled
    );
    assert!(
        !hito
            .community
            .run_next_report_until(pending())
            .await
            .unwrap()
    );
    assert!(primary.requests("v1/audio").await.is_empty());
    assert!(candidate.requests("v1/audio").await.is_empty());
    assert_eq!(owned.private_state().await, before);
    owned.assert_original();
}

async fn assert_default_order(
    hito: &Services,
    actor: &Actor,
    recording: &str,
    baseline: &[Source],
) {
    let sources = hito
        .catalog
        .playback_sources_until(actor, recording, pending())
        .await
        .unwrap();
    assert!(serde_json::to_value(sources).unwrap() == serde_json::to_value(baseline).unwrap());
}

#[tokio::test]
async fn a_held_ranking_body_expires_to_full_source_fallback_and_releases_retry_admission() {
    let primary = provider::ProviderServer::start().await;
    let candidate = provider::ProviderServer::start().await;
    let ranking = remote::RankingServer::start().await;
    let mut reads = JoinSet::new();
    let result = timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(held_body(&primary, &candidate, &ranking, &mut reads)).catch_unwind(),
    )
    .await;
    reads.shutdown().await;
    primary.shutdown().await;
    candidate.shutdown().await;
    ranking.close().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("The owned ranking outage workflow must finish within its deadline"),
    }
}

async fn held_body(
    primary: &provider::ProviderServer,
    candidate: &provider::ProviderServer,
    ranking: &remote::RankingServer,
    reads: &mut JoinSet<hito_core::Result<Vec<Source>>>,
) {
    let owned = library::Library::prepare(primary, candidate, &ranking.url).await;
    let (hito, actor, recording, baseline) =
        (&owned.hito, &owned.actor, &owned.recording, &owned.baseline);
    let before = owned.private_state().await;
    let legacy = hito.community.preferences(actor).await.unwrap();
    let enabled = hito
        .community
        .update_ranking_preferences(
            actor,
            UpdateCommunityRankingPreferences {
                revision: 0,
                use_source_rankings: true,
            },
        )
        .await
        .unwrap();
    ranking.assert_no_requests().await;
    let expected: Vec<_> = baseline.iter().map(library::public_reference).collect();
    let expected_body = json!({"candidates": expected});
    ranking
        .reply(json!({"ranked": [
            {"source": expected[1], "successful_reporters": 9},
            {"source": expected[0], "successful_reporters": 1},
        ]}))
        .await;
    let headers = axum::http::HeaderMap::from_iter([(
        axum::http::header::CONTENT_TYPE,
        "audio/wav".parse().unwrap(),
    )]);
    for provider in [primary, candidate] {
        provider
            .audio(
                axum::http::StatusCode::OK,
                headers.clone(),
                owned.original.clone(),
            )
            .await;
    }
    let counts_before = provider_counts(primary, candidate).await;
    let gate = ranking.hold().await;
    let catalog = hito.catalog.clone();
    let request_actor = actor.clone();
    let id = recording.id.clone();
    let started = Instant::now();
    reads.spawn(async move {
        catalog
            .playback_sources_until(&request_actor, &id, pending())
            .await
    });
    timeout(Duration::from_secs(1), gate.entered())
        .await
        .expect("The ranking response must yield real partial JSON before the deadline");
    assert!(!gate.released());
    ranking
        .assert_one_public_request(expected_body.clone())
        .await;
    let fallback = timeout(Duration::from_millis(500), reads.join_next())
        .await
        .expect("The optional100ms ranking budget must finish while the response body stays held")
        .expect("The owned ranking lookup must remain joined")
        .expect("The ranking lookup must not panic")
        .expect("An unavailable ranking service must preserve source selection");
    let elapsed = started.elapsed();
    assert!(
        (Duration::from_millis(80)..=Duration::from_millis(350)).contains(&elapsed),
        "The100ms optional lookup must retain its bounded wall-clock allowance"
    );
    assert!(
        serde_json::to_value(&fallback).unwrap() == serde_json::to_value(baseline).unwrap(),
        "A held body must return every original source field in deterministic baseline order"
    );
    timeout(Duration::from_millis(500), gate.retired())
        .await
        .expect("The total lookup deadline must drop the real upstream body before release");
    assert!(
        !gate.released(),
        "The fixture must not cause retirement by releasing its body"
    );
    assert!(reads.is_empty());
    assert_eq!(provider_counts(primary, candidate).await, counts_before);
    assert_eq!(owned.private_state().await, before);
    owned.assert_original();
    eprintln!(
        "ranking_held_body elapsed_ms={} retired_before_release=true",
        elapsed.as_millis()
    );

    let selected = &fallback[0];
    let (audio_provider, stream_id) = match selected.label.as_str() {
        "private-youtube-label" => (primary, "private-youtube-audio?signature=canary"),
        "private-jamendo-label" => (candidate, "private-jamendo-audio?signature=canary"),
        _ => panic!("Fallback playback must use only the two owned source representations"),
    };
    timeout(Duration::from_secs(2), async {
        let OpenedAudio::Content(mut audio) = hito
            .catalog
            .open_audio(actor, &selected.id, Default::default())
            .await
            .unwrap()
        else {
            panic!("The owned fallback source must yield provider audio")
        };
        let mut received = Vec::new();
        while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
            assert!(received.len() + chunk.len() <= owned.original.len());
            received.extend_from_slice(&chunk);
        }
        assert_eq!(received, owned.original);
    })
    .await
    .expect("Actual audio must play while the expired community body stays unreleased");
    let audio_requests = audio_provider.requests("v1/audio").await;
    assert_eq!(audio_requests.len(), 1);
    assert!(audio_requests[0].1["source_id"] == stream_id);
    assert!(audio_requests[0].0["authorization"] == "Bearer private-ranking-provider-credential");
    assert_eq!(
        primary.requests("v1/audio").await.len() + candidate.requests("v1/audio").await.len(),
        1
    );
    assert!(!gate.released());
    ranking.release().await;

    let recovered = hito
        .catalog
        .playback_sources_until(actor, &recording.id, pending())
        .await
        .unwrap();
    assert!(
        serde_json::to_value(&recovered).unwrap()
            == serde_json::to_value(baseline.iter().rev().collect::<Vec<_>>()).unwrap(),
        "A fresh lookup must regain admission and use the current exact-set response"
    );
    ranking
        .assert_public_requests(&[expected_body.clone(), expected_body])
        .await;
    let mut counts_after_audio = counts_before;
    let last = counts_after_audio.len() - 1;
    counts_after_audio[last] = 1;
    assert_eq!(
        provider_counts(primary, candidate).await,
        counts_after_audio
    );
    assert_eq!(hito.community.preferences(actor).await.unwrap(), legacy);
    assert_eq!(
        hito.community.ranking_preferences(actor).await.unwrap(),
        enabled
    );
    assert_eq!(owned.private_state().await, before);
    owned.assert_original();
}

async fn provider_counts(
    primary: &provider::ProviderServer,
    candidate: &provider::ProviderServer,
) -> Vec<usize> {
    let mut counts = Vec::new();
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
    ] {
        for provider in [primary, candidate] {
            counts.push(provider.requests(path).await.len());
        }
    }
    counts.push(
        primary.requests("v1/audio").await.len() + candidate.requests("v1/audio").await.len(),
    );
    counts
}
