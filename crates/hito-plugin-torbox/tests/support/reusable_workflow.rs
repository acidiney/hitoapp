use super::{fixtures, http, logs, provider, scripted_media, state_inputs};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, ProviderRecording, SourcePage,
};
use hito_plugin_torbox::{Config, CuratedWavRecording};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;
use uuid::Uuid;

pub const COOKIE: &str = "owned-private-torbox-reuse-cookie";
pub const SERVICE_TOKEN: &str = "owned-private-torbox-reuse-bearer";
const RECORDING: &str = "curated-reusable-recording";
const SOURCE: &str = "curated-reusable-source";

pub struct Saved {
    pub request: PreparationRequest,
    pub receipt: PreparationStatusRequest,
}

pub struct Workflow {
    pub client: reqwest::Client,
    pub service: state_inputs::Server,
    pub provider: provider::Fixture,
    pub media: scripted_media::ScriptedMedia,
    pub capture: logs::Capture,
}

impl Workflow {
    pub async fn start(
        replies: Vec<provider::reply::Reply>,
        audio: Vec<scripted_media::Reply>,
    ) -> Self {
        let media = scripted_media::ScriptedMedia::start(audio).await;
        let provider = provider::Fixture::responses(replies).await;
        let capture = logs::Capture::default();
        let inputs = state_inputs::Inputs::new();
        let service = state_inputs::Server::start(
            Config {
                state: inputs.next(),
                service_bearer: SERVICE_TOKEN.into(),
                client: TorBoxClient::with_test_tls_media(
                    fixtures::TOKEN.into(),
                    provider.ca_der(),
                    provider.address(),
                    media.ca_der(),
                    media.address(),
                )
                .unwrap(),
                media_origins: MediaAllowlist::new(vec![format!(
                    "https://{}",
                    fixtures::MEDIA_HOST
                )])
                .unwrap(),
                recordings: vec![CuratedWavRecording {
                    recording: ProviderRecording {
                        id: RECORDING.into(),
                        title: "owned-private-reuse-title".into(),
                        artist: "owned-private-reuse-artist".into(),
                        duration_ms: Some(
                            fixtures::FRAMES * 1000 / u64::from(fixtures::SAMPLE_RATE),
                        ),
                        release: None,
                        disc_number: None,
                        track_number: None,
                        revision: None,
                        attribution: None,
                    },
                    source_id: SOURCE.into(),
                    label: "Curated canonical PCM16 WAV".into(),
                    binding: fixtures::binding(),
                }],
            },
            capture.dispatch(),
            Some(inputs),
        )
        .await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap();
        Self {
            client,
            service,
            provider,
            media,
            capture,
        }
    }

    pub async fn prepare(&self) -> Saved {
        let request = PreparationRequest {
            operation_id: Uuid::new_v4(),
            recording_id: RECORDING.into(),
            source_id: SOURCE.into(),
        };
        let submitted: PreparationSubmitResponse = self.post("v1/sources/prepare", &request).await;
        submitted.validate(&request).unwrap();
        let PreparationSubmission::Accepted {
            handle,
            status: PreparationStatus::Ready {},
        } = submitted.submission
        else {
            panic!("the completed exact selection must retain a ready candidate")
        };
        let receipt = PreparationStatusRequest {
            operation_id: request.operation_id,
            recording_id: RECORDING.into(),
            source_id: SOURCE.into(),
            handle,
        };
        Saved { request, receipt }
    }

    pub fn audio_request(&self) -> reqwest::RequestBuilder {
        self.client
            .get(
                self.service
                    .base
                    .join(&format!("v1/audio?source_id={SOURCE}"))
                    .unwrap(),
            )
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", COOKIE)
    }

    pub async fn audio(&self) -> reqwest::Response {
        let response = self.audio_request().send().await.unwrap();
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        response
    }

    pub async fn assert_ready(&self, saved: &Saved) {
        let replay: PreparationSubmitResponse =
            self.post("v1/sources/prepare", &saved.request).await;
        replay.validate(&saved.request).unwrap();
        assert!(
            matches!(replay.submission, PreparationSubmission::Accepted { handle, status: PreparationStatus::Ready {} } if handle == saved.receipt.handle)
        );
        let status: PreparationStatusResponse = self
            .post("v1/sources/preparation/status", &saved.receipt)
            .await;
        status.validate(&saved.receipt).unwrap();
        assert!(matches!(status.status, PreparationStatus::Ready {}));
    }

    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        request: &impl serde::Serialize,
    ) -> T {
        let (status, bytes) = http::json(
            &self.client,
            &self.service,
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

    pub async fn sources(&self) -> bool {
        let page: SourcePage = self
            .post("v1/sources", &serde_json::json!({"recording_id":RECORDING}))
            .await;
        page.validate().unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].id == SOURCE);
        page.items[0].ready
    }

    pub async fn close(self) -> (Vec<provider::Request>, usize, logs::Capture) {
        self.service.close().await.unwrap();
        let requests = self.provider.close().await;
        let media = self.media.close().await;
        (requests, media, self.capture)
    }
}

pub async fn complete(mut response: reqwest::Response) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert!(response.headers()["content-length"] == fixtures::WAV_BYTES.to_string());
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let mut received = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(chunk.len() <= (fixtures::WAV_BYTES as usize).saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert!(received == fixtures::wave());
    fixtures::assert_pcm(received);
}

pub fn assert_requests(requests: &[provider::Request], count: usize) {
    assert_eq!(requests.len(), count);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "GET" && request.server_name == "api.torbox.app");
        assert!(request.header("cookie").is_none());
        assert!(request.header("x-hito-plugin-protocol").is_none());
        assert!(request.body.is_empty());
        if index == 1 {
            assert!(
                request.target
                    == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false"
            );
            assert!(request.header("authorization").is_none());
        } else {
            assert!(request.target == "/v1/api/torrents/mylist?id=7123&bypass_cache=true");
            assert!(
                request.header("authorization")
                    == Some(format!("Bearer {}", fixtures::TOKEN).as_str())
            );
        }
    }
}

pub fn private_json(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    for private in [
        fixtures::TOKEN,
        SERVICE_TOKEN,
        COOKIE,
        fixtures::MEDIA_TARGET,
        "album/owned.wav",
    ] {
        assert!(!text.contains(private));
    }
}
