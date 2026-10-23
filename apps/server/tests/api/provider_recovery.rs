use super::provider_fixture::ProviderServer;
use super::support::{admin, call, services};
use axum::http::StatusCode;
use hito_core::{AdoptProviderRecording, AttachReviewedProviderSource, ProviderCandidate};
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn playback_refresh_purpose_recovers_without_waiting_for_a_stalled_binding() {
    let providers = [ProviderServer::start().await, ProviderServer::start().await];
    let directory = tempfile::tempdir().unwrap();
    let hito = services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let actor = hito.identity.authenticate(&token).await.unwrap();
    let mut installations = Vec::new();
    for (index, provider) in providers.iter().enumerate() {
        provider.reply("v1/recordings/detail", json!({
            "id": format!("item-{index}"), "title": format!("Owned track {index}"), "artist": "Owned artist"
        })).await;
        provider.reply("v1/sources", json!({"items": [{
            "id": format!("old-{index}"), "label": "Previously accepted source", "ready": true
        }]})).await;
        let (status, service) = call(
            &app,
            "POST",
            "/api/v1/plugins/services",
            json!({
                "name": "Owned recovery provider", "base_url": provider.url
            }),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, installation) = call(&app, "POST", "/api/v1/plugins", json!({
            "service_id": service["id"], "service_revision": service["revision"], "bearer_token": null
        }), Some(&token)).await;
        assert_eq!(status, StatusCode::CREATED);
        installations.push((
            installation["id"].as_str().unwrap().to_owned(),
            installation["revision"].as_i64().unwrap(),
        ));
    }
    let recording = hito
        .catalog
        .adopt_provider(
            &actor,
            &installations[0].0,
            AdoptProviderRecording {
                installation_revision: installations[0].1,
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
        installation_id: installations[1].0.clone(),
        installation_revision: installations[1].1,
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
    let sources_path = format!("/api/v1/recordings/{}/sources", recording.id);
    let refresh_path = format!("{sources_path}/refresh");
    let gates = [
        providers[0].hold("v1/sources").await,
        providers[1].hold("v1/sources").await,
    ];
    let mut pending = {
        let app = app.clone();
        let token = token.clone();
        tokio::spawn(async move {
            call(
                &app,
                "POST",
                &refresh_path,
                json!({"purpose": "playback"}),
                Some(&token),
            )
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
    .expect("the HTTP refresh must contact one owned provider");
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
        "playback-purpose HTTP refresh must contact the healthy binding while another provider is stalled"
    );
    gates[healthy].release();
    let completed = tokio::time::timeout(Duration::from_secs(2), &mut pending).await;
    if completed.is_err() {
        gates[stalled].open();
        let _ = tokio::time::timeout(Duration::from_secs(3), &mut pending).await;
    }
    let (status, published) = completed
        .expect("playback-purpose HTTP refresh must finish after a ready publication")
        .unwrap();
    assert_eq!(status, StatusCode::OK);
    let sources = published.as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| source["ready"] == true));
    assert!(
        sources
            .iter()
            .all(|source| source["recording_id"].as_str() == Some(recording.id.as_str()))
    );
    assert!(
        sources
            .iter()
            .any(|source| source["id"].as_str() == Some(old_sources[stalled].as_str()))
    );
    assert!(
        sources
            .iter()
            .all(|source| source["id"].as_str() != Some(old_sources[healthy].as_str()))
    );
    let (status, cached) = call(&app, "GET", &sources_path, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cached, published);
    gates[stalled].open();
}
