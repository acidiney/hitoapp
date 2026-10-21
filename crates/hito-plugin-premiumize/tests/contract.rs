#[path = "fixtures/http.rs"]
mod http;
#[path = "fixtures/logging.rs"]
mod logging;
#[path = "fixtures/provider.rs"]
mod provider;
#[path = "../../hito-plugin-real-debrid/tests/support/audio.rs"]
pub mod wave_fixture;
#[path = "../../hito-debrid-premiumize/tests/audio_fixture/mod.rs"]
pub mod wire_fixture;

use futures_util::{FutureExt, future};
use hito_plugin_premiumize::{Config, router};
use hito_plugin_sdk::{
    Capability, Manifest, PreparationRequest, PreparationStatus, PreparationStatusRequest,
    PreparationStatusResponse, PreparationSubmission, PreparationSubmitResponse, ProviderRecording,
    SearchPage, SourcePage, SourcePersistence,
};
use provider::*;
use reqwest::StatusCode;
use serde_json::json;
use std::{panic::AssertUnwindSafe, time::Duration};

#[tokio::test]
async fn account_owned_catalog_monitors_an_existing_cloud_transfer_and_relays_fresh_exact_audio() {
    let provider = Provider::start().await;
    let capture = logging::Capture::default();
    let mut plugin = None;
    let outcome = AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(30), async {
        let app = router(Config {
            connections: provider.connections(),
        })
        .expect("the two private curated connections must initialize");
        plugin = Some(http::Server::start(app, capture.dispatch()).await);
        workflow(plugin.as_ref().unwrap(), &provider).await
    }))
    .catch_unwind()
    .await;
    let close_plugin = async {
        match plugin {
            Some(plugin) => plugin.close().await,
            None => Ok(()),
        }
    };
    let (plugin_stopped, exchanges) = future::join(close_plugin, provider.close()).await;
    plugin_stopped.expect("the owned plugin HTTP server must stop");
    let (receipts, audio) = match outcome {
        Ok(result) => result.expect("the complete public plugin journey exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        audio == wave_fixture::wav(),
        "the relay changed the exact owned WAV"
    );
    wave_fixture::assert_decoded(audio);
    assert_protocol(exchanges);
    capture.assert_complete(&receipts, wave_fixture::WAV_BYTES as u64);
}

async fn workflow(plugin: &http::Server, provider: &Provider) -> (Vec<http::Receipt>, Vec<u8>) {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap();
    let mut receipts = Vec::new();
    let bytes = http::json(
        &client,
        plugin,
        "v1/manifest",
        None,
        None,
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.id, "org.hito.premiumize");
    assert!(
        manifest.capabilities
            == [
                Capability::Search,
                Capability::Sources,
                Capability::SourcePreparation
            ]
            .into()
    );
    assert!(manifest.authorization.is_none());
    assert_eq!(
        manifest.content_policy.unwrap().source_persistence,
        SourcePersistence::PlaybackOnly
    );
    assert_eq!(provider.counts(), [0, 0, 0, 0]);

    for token in [None, Some(WRONG_SERVICE)] {
        let bytes = http::json(
            &client,
            plugin,
            "v1/search",
            token,
            Some(json!({"query":QUERY, "cursor":null, "limit":10})),
            StatusCode::UNAUTHORIZED,
            &mut receipts,
        )
        .await;
        assert!(
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .unwrap()
                .get("error")
                .is_some()
        );
    }
    assert_eq!(provider.counts(), [0, 0, 0, 0]);
    let bytes = http::json(
        &client,
        plugin,
        "v1/search",
        Some(SERVICE),
        Some(json!({"query":QUERY, "cursor":null, "limit":10})),
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    page.validate(10).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0] == recording(false));
    assert!(page.next_cursor.is_none());
    let bytes = http::json(
        &client,
        plugin,
        "v1/recordings/detail",
        Some(SERVICE),
        Some(json!({"recording_id":RECORDING})),
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let detail: ProviderRecording = serde_json::from_slice(&bytes).unwrap();
    detail.validate().unwrap();
    assert!(detail == recording(false));
    let bytes = http::json(
        &client,
        plugin,
        "v1/search",
        Some(PEER_SERVICE),
        Some(json!({"query":"owned-private-peer", "cursor":null, "limit":10})),
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let peer: SearchPage = serde_json::from_slice(&bytes).unwrap();
    peer.validate(10).unwrap();
    assert_eq!(peer.items.len(), 1);
    assert!(peer.items[0] == recording(true));
    http::json(
        &client,
        plugin,
        "v1/recordings/detail",
        Some(PEER_SERVICE),
        Some(json!({"recording_id":RECORDING})),
        StatusCode::NOT_FOUND,
        &mut receipts,
    )
    .await;
    assert_eq!(provider.counts(), [0, 0, 0, 0]);

    let bytes = http::json(
        &client,
        plugin,
        "v1/sources",
        Some(SERVICE),
        Some(json!({"recording_id":RECORDING})),
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let sources: SourcePage = serde_json::from_slice(&bytes).unwrap();
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].id == SOURCE && sources.items[0].label == LABEL);
    assert!(
        !sources.items[0].ready,
        "queued transfers must not become playable sources"
    );
    assert_eq!(provider.counts(), [1, 0, 0, 0]);

    let request = PreparationRequest {
        operation_id: uuid::Uuid::new_v4(),
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
    };
    let bytes = http::json(
        &client,
        plugin,
        "v1/sources/prepare",
        Some(SERVICE),
        Some(serde_json::to_value(&request).unwrap()),
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let submitted: PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
    submitted.validate(&request).unwrap();
    let handle = match submitted.submission {
        PreparationSubmission::Accepted {
            handle,
            status: PreparationStatus::Queued { poll_after_seconds },
        } => {
            assert_eq!(poll_after_seconds, 5);
            handle
        }
        _ => panic!("the service must report only the observed queued transfer"),
    };
    assert!(!handle.is_empty() && handle.len() <= hito_plugin_sdk::MAX_PREPARATION_HANDLE_BYTES);
    assert_eq!(provider.counts(), [2, 0, 0, 0]);

    let status = PreparationStatusRequest {
        operation_id: request.operation_id,
        recording_id: request.recording_id.clone(),
        source_id: request.source_id.clone(),
        handle,
    };
    let bytes = http::json(
        &client,
        plugin,
        "v1/sources/preparation/status",
        Some(SERVICE),
        Some(serde_json::to_value(&status).unwrap()),
        StatusCode::OK,
        &mut receipts,
    )
    .await;
    let observed: PreparationStatusResponse = serde_json::from_slice(&bytes).unwrap();
    observed.validate(&status).unwrap();
    assert!(matches!(observed.status, PreparationStatus::Ready {}));
    assert_eq!(
        provider.counts(),
        [5, 0, 0, 0],
        "readiness must verify the exact file without opening its private link"
    );

    let audio = http::audio(&client, plugin, &mut receipts).await;
    assert_eq!(
        provider.counts(),
        [7, 1, 0, 0],
        "explicit audio must perform fresh authority reads and one credential-free media GET"
    );
    assert_eq!(audio.len(), wave_fixture::WAV_BYTES);
    assert_eq!(receipts.len(), 11);
    (receipts, audio)
}
