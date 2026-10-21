use super::{COOKIE, SERVICE_TOKEN, fixtures, http};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, ProviderRecording, SourcePage,
};
use hito_plugin_torbox::CuratedWavRecording;
use reqwest::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

pub fn selection(index: usize) -> CuratedWavRecording {
    let mut binding = fixtures::binding();
    binding.torrent_id += index as u64;
    binding.info_hash = format!("{:040x}", index + 1);
    CuratedWavRecording {
        recording: ProviderRecording {
            id: format!("curated-admission-recording-{index}"),
            title: "owned-private-admission-title".into(),
            artist: "owned-private-admission-artist".into(),
            duration_ms: Some(fixtures::FRAMES * 1000 / u64::from(fixtures::SAMPLE_RATE)),
            release: None,
            disc_number: None,
            track_number: None,
            revision: None,
            attribution: None,
        },
        source_id: format!("curated-admission-source-{index}"),
        label: "Curated canonical PCM16 WAV".into(),
        binding,
    }
}

pub async fn prepare(
    client: &reqwest::Client,
    server: &http::Server,
    index: usize,
) -> (PreparationRequest, PreparationStatusRequest) {
    let entry = selection(index);
    let request = PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: entry.recording.id,
        source_id: entry.source_id,
    };
    let submitted: PreparationSubmitResponse =
        post(client, server, "v1/sources/prepare", &request).await;
    submitted.validate(&request).unwrap();
    let PreparationSubmission::Accepted {
        handle,
        status: PreparationStatus::Ready {},
    } = submitted.submission
    else {
        panic!("the completed owned selection must mint a ready candidate")
    };
    let receipt = PreparationStatusRequest {
        operation_id: request.operation_id,
        recording_id: request.recording_id.clone(),
        source_id: request.source_id.clone(),
        handle,
    };
    (request, receipt)
}

pub async fn replay(
    client: &reqwest::Client,
    server: &http::Server,
    request: &PreparationRequest,
    receipt: &PreparationStatusRequest,
) {
    let replay: PreparationSubmitResponse =
        post(client, server, "v1/sources/prepare", request).await;
    replay.validate(request).unwrap();
    assert!(
        matches!(replay.submission, PreparationSubmission::Accepted { handle, status: PreparationStatus::Ready {} } if handle == receipt.handle),
        "admission rejection must preserve the same ready operation and private candidate"
    );
    let status: PreparationStatusResponse =
        post(client, server, "v1/sources/preparation/status", receipt).await;
    status.validate(receipt).unwrap();
    assert!(matches!(status.status, PreparationStatus::Ready {}));
}

pub async fn ready(client: &reqwest::Client, server: &http::Server, index: usize) -> bool {
    let page: SourcePage = post(
        client,
        server,
        "v1/sources",
        &serde_json::json!({"recording_id":selection(index).recording.id}),
    )
    .await;
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].id == selection(index).source_id);
    page.items[0].ready
}

pub async fn audio(
    client: &reqwest::Client,
    server: &http::Server,
    index: usize,
) -> reqwest::Response {
    audio_request(client, server, index, Method::GET).await
}

pub async fn head(client: &reqwest::Client, server: &http::Server, index: usize) {
    let response = audio_request(client, server, index, Method::HEAD).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert!(response.headers()["content-length"] == fixtures::WAV_BYTES.to_string());
    assert!(response.bytes().await.unwrap().is_empty());
}

async fn audio_request(
    client: &reqwest::Client,
    server: &http::Server,
    index: usize,
    method: Method,
) -> reqwest::Response {
    let mut url = server.base.join("v1/audio").unwrap();
    url.query_pairs_mut()
        .append_pair("source_id", &selection(index).source_id);
    let response = client
        .request(method, url)
        .bearer_auth(SERVICE_TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", COOKIE)
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    response
}

async fn post<T: DeserializeOwned>(
    client: &reqwest::Client,
    server: &http::Server,
    path: &str,
    request: &impl Serialize,
) -> T {
    let (status, bytes) = http::json(
        client,
        server,
        Method::POST,
        path,
        Some(SERVICE_TOKEN),
        Some(serde_json::to_value(request).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    private_json(&bytes);
    serde_json::from_slice(&bytes).unwrap()
}

pub fn private_json(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    for forbidden in [
        fixtures::TOKEN,
        SERVICE_TOKEN,
        COOKIE,
        "album/owned.wav",
        fixtures::MEDIA_TARGET,
        "api.torbox.app",
        "\"torrent_id\"",
        "\"file_id\"",
        "http://",
        "https://",
    ] {
        let escaped = serde_json::to_string(forbidden).unwrap();
        assert!(!text.contains(forbidden) && !text.contains(&escaped[1..escaped.len() - 1]));
    }
}
