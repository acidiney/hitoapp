#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use axum::http::{HeaderMap, StatusCode};
use hito_core::{
    Actor, AdoptProviderRecording, AttachReviewedProviderSource, InstallPlugin, OfferPluginService,
    OpenedAudio, PluginInstallation, ProviderCandidate, Services,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn automatic_refresh_uses_a_ready_binding_without_waiting_for_a_stalled_provider() {
    let providers = [
        fixture::ProviderServer::start().await,
        fixture::ProviderServer::start().await,
    ];
    let (directory, hito, actor, _library, _music) = support::library("binding-refresh").await;
    let audio = [
        support::write_wav(&directory.path().join("first.wav"), 10),
        support::write_wav(&directory.path().join("second.wav"), 20),
    ];
    for index in 0..2 {
        providers[index].reply("v1/recordings/detail", json!({
            "id": format!("item-{index}"), "title": format!("Owned track {index}"), "artist": "Owned artist"
        })).await;
        providers[index].reply("v1/sources", json!({"items": [{
            "id": format!("old-{index}"), "label": "Previously accepted source", "ready": true
        }]})).await;
        providers[index]
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([(
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                )]),
                audio[index].clone(),
            )
            .await;
    }
    let first = connect(&hito, &actor, &providers[0]).await;
    let second = connect(&hito, &actor, &providers[1]).await;
    let recording = hito
        .catalog
        .adopt_provider(
            &actor,
            &first.id,
            AdoptProviderRecording {
                installation_revision: first.revision,
                provider_recording_id: "item-0".into(),
            },
        )
        .await
        .unwrap();
    let first_source = hito
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    let candidate = ProviderCandidate {
        installation_id: second.id,
        installation_revision: second.revision,
        provider_recording_id: "item-1".into(),
    };
    let review = hito
        .catalog
        .review_provider_source(&actor, &recording.id, candidate.clone())
        .await
        .unwrap();
    hito.catalog
        .attach_provider_source(
            &actor,
            &recording.id,
            AttachReviewedProviderSource {
                candidate,
                review_stamp: review.review_stamp,
            },
        )
        .await
        .unwrap();
    let accepted = hito.catalog.sources(&actor, &recording.id).await.unwrap();
    assert_eq!(accepted.len(), 2);
    let second_source = accepted
        .iter()
        .find(|source| source.id != first_source)
        .unwrap()
        .id
        .clone();
    let old_sources = [first_source, second_source];
    for (index, provider) in providers.iter().enumerate() {
        provider.reply("v1/sources", json!({"items": [{
            "id": format!("replacement-{index}"), "label": "Replacement source", "ready": true
        }]})).await;
    }
    let gates = [
        providers[0].hold("v1/sources").await,
        providers[1].hold("v1/sources").await,
    ];
    let mut pending = {
        let catalog = hito.catalog.clone();
        let actor = actor.clone();
        let recording = recording.id.clone();
        tokio::spawn(async move {
            catalog
                .refresh_playback_sources_until(&actor, &recording, std::future::pending())
                .await
        })
    };
    let stalled = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            _ = gates[0].entered() => 0,
            _ = gates[1].entered() => 1,
        }
    })
    .await
    .expect("one provider must receive the first refresh request");
    let healthy = 1 - stalled;
    let second_started =
        tokio::time::timeout(Duration::from_secs(2), gates[healthy].entered()).await;
    if second_started.is_err() {
        for gate in &gates {
            gate.open();
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), &mut pending).await;
    }
    assert!(
        second_started.is_ok(),
        "a healthy provider must be contacted while another binding is stalled"
    );
    gates[healthy].release();
    let completed = tokio::time::timeout(Duration::from_secs(2), &mut pending).await;
    if completed.is_err() {
        gates[stalled].open();
        let _ = tokio::time::timeout(Duration::from_secs(3), &mut pending).await;
    }
    assert!(
        completed
            .expect("automatic refresh must finish promptly after a ready publication")
            .unwrap()
            .unwrap()
    );
    let sources = hito.catalog.sources(&actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| source.ready));
    assert!(
        sources
            .iter()
            .any(|source| source.id == old_sources[stalled])
    );
    assert!(
        sources
            .iter()
            .all(|source| source.id != old_sources[healthy])
    );
    let replacement = sources
        .iter()
        .find(|source| source.id != old_sources[stalled])
        .unwrap();
    let grant = hito
        .catalog
        .playback_source(&actor, &recording.id, &replacement.id)
        .await
        .unwrap();
    let OpenedAudio::Content(mut content) = grant.open(Default::default()).await.unwrap() else {
        panic!("the independently refreshed binding must provide its owned audio");
    };
    let mut body = content.body.take().unwrap();
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, audio[healthy]);
    let requests = providers[healthy].requests("v1/audio").await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].1["source_id"], format!("replacement-{healthy}"));
    assert!(providers[stalled].requests("v1/audio").await.is_empty());
    let published = serde_json::to_value(sources).unwrap();
    gates[stalled].open();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        serde_json::to_value(hito.catalog.sources(&actor, &recording.id).await.unwrap()).unwrap(),
        published,
        "an unfinished response must not publish after automatic recovery has completed"
    );
}

async fn connect(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned refresh fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}
