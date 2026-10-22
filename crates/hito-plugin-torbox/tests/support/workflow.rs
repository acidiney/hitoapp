use super::{
    ARTIST, QUERY, RECORDING, SERVICE_TOKEN, SOURCE, WRONG_TOKEN, http, private_json, recording,
};
use hito_plugin_sdk::{
    Capability, Manifest, PreparationRequest, PreparationStatus, PreparationStatusRequest,
    PreparationStatusResponse, PreparationSubmission, PreparationSubmitResponse, ProviderRecording,
    SearchPage, SourcePage, SourcePersistence,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

pub async fn discover(client: &reqwest::Client, plugin: &http::Server, statuses: &mut Vec<u16>) {
    let (status, body) = http::json(
        client,
        plugin,
        Method::GET,
        "v1/manifest",
        super::COOKIE,
        None,
        None,
    )
    .await;
    statuses.push(status.as_u16());
    assert_eq!(status, StatusCode::OK);
    private_json(&body);
    let manifest: Manifest = serde_json::from_slice(&body).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.id, "org.hito.torbox");
    assert!(
        manifest.capabilities
            == [
                Capability::Search,
                Capability::Sources,
                Capability::SourcePreparation
            ]
            .into()
    );
    assert_eq!(
        manifest.content_policy.unwrap().source_persistence,
        SourcePersistence::PlaybackOnly
    );
    for token in [None, Some(WRONG_TOKEN)] {
        let (status, body) = http::json(
            client,
            plugin,
            Method::POST,
            "v1/search",
            super::COOKIE,
            token,
            Some(json!({"query":QUERY,"cursor":null,"limit":10})),
        )
        .await;
        statuses.push(status.as_u16());
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        private_json(&body);
    }
    let body = post(
        client,
        plugin,
        statuses,
        "v1/search",
        Some(json!({"query":QUERY,"cursor":null,"limit":10})),
        StatusCode::OK,
    )
    .await;
    let page: SearchPage = serde_json::from_slice(&body).unwrap();
    page.validate(10).unwrap();
    assert!(page.items == vec![recording()] && page.next_cursor.is_none());
    let body = post(
        client,
        plugin,
        statuses,
        "v1/recordings/detail",
        Some(json!({"recording_id":RECORDING})),
        StatusCode::OK,
    )
    .await;
    let detail: ProviderRecording = serde_json::from_slice(&body).unwrap();
    detail.validate().unwrap();
    assert!(detail == recording() && detail.artist == ARTIST);
}

pub async fn sources(
    client: &reqwest::Client,
    plugin: &http::Server,
    statuses: &mut Vec<u16>,
) -> bool {
    let body = post(
        client,
        plugin,
        statuses,
        "v1/sources",
        Some(json!({"recording_id":RECORDING})),
        StatusCode::OK,
    )
    .await;
    let page: SourcePage = serde_json::from_slice(&body).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].id == SOURCE && page.items[0].label == "Curated canonical PCM16 WAV");
    page.items[0].ready
}

pub async fn prepare_and_poll(
    client: &reqwest::Client,
    plugin: &http::Server,
    statuses: &mut Vec<u16>,
) -> PreparationStatusRequest {
    let prepare = PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
    };
    let body = post(
        client,
        plugin,
        statuses,
        "v1/sources/prepare",
        Some(serde_json::to_value(&prepare).unwrap()),
        StatusCode::OK,
    )
    .await;
    let submitted: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    submitted.validate(&prepare).unwrap();
    let PreparationSubmission::Accepted { handle, status } = submitted.submission else {
        panic!(
            "an incomplete exact selection must retain a preparation operation without minting a link"
        )
    };
    preparing(&status);
    let receipt = PreparationStatusRequest {
        operation_id: prepare.operation_id,
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
        handle,
    };
    receipt.validate().unwrap();
    private_json(&serde_json::to_vec(&receipt).unwrap());
    let body = post(
        client,
        plugin,
        statuses,
        "v1/sources/prepare",
        Some(serde_json::to_value(&prepare).unwrap()),
        StatusCode::OK,
    )
    .await;
    let replay: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    replay.validate(&prepare).unwrap();
    match replay.submission {
        PreparationSubmission::Accepted { handle, status } => {
            assert!(handle == receipt.handle);
            preparing(&status);
        }
        _ => panic!("the same operation must retain its original receipt"),
    }
    let mut forged = serde_json::to_value(&receipt).unwrap();
    forged["operation_id"] = json!(Uuid::new_v4());
    post(
        client,
        plugin,
        statuses,
        "v1/sources/preparation/status",
        Some(forged),
        StatusCode::NOT_FOUND,
    )
    .await;
    let body = post(
        client,
        plugin,
        statuses,
        "v1/sources/preparation/status",
        Some(serde_json::to_value(&receipt).unwrap()),
        StatusCode::OK,
    )
    .await;
    let early: PreparationStatusResponse = serde_json::from_slice(&body).unwrap();
    early.validate(&receipt).unwrap();
    preparing(&early.status);
    tokio::time::sleep(Duration::from_secs(5)).await;
    let body = post(
        client,
        plugin,
        statuses,
        "v1/sources/preparation/status",
        Some(serde_json::to_value(&receipt).unwrap()),
        StatusCode::OK,
    )
    .await;
    let ready: PreparationStatusResponse = serde_json::from_slice(&body).unwrap();
    ready.validate(&receipt).unwrap();
    assert!(matches!(ready.status, PreparationStatus::Ready {}));
    let body = post(
        client,
        plugin,
        statuses,
        "v1/sources/prepare",
        Some(serde_json::to_value(&prepare).unwrap()),
        StatusCode::OK,
    )
    .await;
    let replay: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    replay.validate(&prepare).unwrap();
    assert!(
        matches!(replay.submission, PreparationSubmission::Accepted { handle, status: PreparationStatus::Ready {} } if handle == receipt.handle)
    );
    receipt
}

fn preparing(status: &PreparationStatus) {
    assert!(matches!(
        status,
        PreparationStatus::Preparing {
            progress_basis_points: None,
            eta_seconds: None,
            poll_after_seconds: 5,
        }
    ));
}

async fn post(
    client: &reqwest::Client,
    plugin: &http::Server,
    statuses: &mut Vec<u16>,
    path: &str,
    body: Option<Value>,
    expected: StatusCode,
) -> Vec<u8> {
    let (status, bytes) = http::json(
        client,
        plugin,
        Method::POST,
        path,
        super::COOKIE,
        Some(SERVICE_TOKEN),
        body,
    )
    .await;
    statuses.push(status.as_u16());
    assert_eq!(status, expected);
    private_json(&bytes);
    bytes
}
