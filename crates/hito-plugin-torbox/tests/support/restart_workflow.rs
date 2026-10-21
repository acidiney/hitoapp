use super::{fixtures, http, provider, scripted_media, state_inputs};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, ProviderRecording,
};
use hito_plugin_torbox::{Config, CuratedWavRecording};
use provider::reply::Reply;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;
use uuid::Uuid;

pub const COOKIE: &str = "owned-private-torbox-restart-cookie";
const SERVICE_TOKEN: &str = "owned-private-torbox-restart-bearer";
const RECORDING: &str = "curated-restart-recording";
const SOURCE: &str = "curated-restart-source";

pub struct Saved {
    request: PreparationRequest,
    receipt: PreparationStatusRequest,
}

pub struct Fixture {
    provider: provider::Fixture,
    media: scripted_media::ScriptedMedia,
    inputs: state_inputs::Inputs,
}

pub struct Service {
    client: reqwest::Client,
    server: state_inputs::Server,
}

impl Fixture {
    pub async fn start() -> Self {
        let observed = || Reply::json(fixtures::observed(true, true, "album/owned.wav"));
        let provider = provider::Fixture::responses(vec![
            observed(),
            Reply::json(fixtures::acknowledgment()),
            observed(),
            observed(),
        ])
        .await;
        let media = scripted_media::ScriptedMedia::start(vec![
            scripted_media::Reply::full(fixtures::wave()),
            scripted_media::Reply::full(fixtures::wave()),
        ])
        .await;
        Self {
            provider,
            media,
            inputs: state_inputs::Inputs::new(),
        }
    }

    pub async fn start_service(&self) -> Service {
        let server = state_inputs::Server::start(
            Config {
                state: self.inputs.next(),
                service_bearer: SERVICE_TOKEN.into(),
                client: TorBoxClient::with_test_tls_media(
                    fixtures::TOKEN.into(),
                    self.provider.ca_der(),
                    self.provider.address(),
                    self.media.ca_der(),
                    self.media.address(),
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
                        title: "owned-private-restart-title".into(),
                        artist: "owned-private-restart-artist".into(),
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
            tracing::Dispatch::none(),
            None,
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
        Service { client, server }
    }

    pub async fn close(self) -> (Vec<provider::Request>, usize) {
        let requests = self.provider.close().await;
        let media_requests = self.media.close().await;
        (requests, media_requests)
    }
}

impl Service {
    pub async fn prepare(&self) -> Saved {
        let request = PreparationRequest {
            operation_id: Uuid::new_v4(),
            recording_id: RECORDING.into(),
            source_id: SOURCE.into(),
        };
        let response: PreparationSubmitResponse = self.post("v1/sources/prepare", &request).await;
        response.validate(&request).unwrap();
        let PreparationSubmission::Accepted {
            handle,
            status: PreparationStatus::Ready {},
        } = response.submission
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

    pub async fn status(&self, saved: &Saved) -> (StatusCode, Vec<u8>) {
        self.post_raw("v1/sources/preparation/status", &saved.receipt)
            .await
    }

    pub async fn assert_replay_ready(&self, saved: &Saved) {
        let response: PreparationSubmitResponse =
            self.post("v1/sources/prepare", &saved.request).await;
        response.validate(&saved.request).unwrap();
        assert!(matches!(
            response.submission,
            PreparationSubmission::Accepted {
                handle,
                status: PreparationStatus::Ready {},
            } if handle == saved.receipt.handle
        ));
    }

    pub async fn audio(&self) -> reqwest::Response {
        self.client
            .get(
                self.server
                    .base
                    .join(&format!("v1/audio?source_id={SOURCE}"))
                    .unwrap(),
            )
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", COOKIE)
            .send()
            .await
            .unwrap()
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, request: &impl serde::Serialize) -> T {
        let (status, bytes) = self.post_raw(path, request).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn post_raw(&self, path: &str, request: &impl serde::Serialize) -> (StatusCode, Vec<u8>) {
        http::json(
            &self.client,
            &self.server,
            Method::POST,
            path,
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(request).unwrap()),
        )
        .await
    }

    pub async fn close(self) {
        let Self { client, server } = self;
        drop(client);
        server.close().await.unwrap();
    }
}

pub fn assert_ready_status(bytes: &[u8], saved: &Saved) {
    let response: PreparationStatusResponse = serde_json::from_slice(bytes).unwrap();
    response.validate(&saved.receipt).unwrap();
    assert!(matches!(response.status, PreparationStatus::Ready {}));
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

pub fn assert_requests(
    requests: &[provider::Request],
    get_windows: [(tokio::time::Instant, tokio::time::Instant); 2],
) {
    assert_eq!(requests.len(), 4);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "GET" && request.server_name == "api.torbox.app");
        assert!(request.body.is_empty());
        for name in [
            "cookie",
            "referer",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(name).is_none());
        }
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
            if index >= 2 {
                let (started, finished) = get_windows[index - 2];
                assert!((started..=finished).contains(&request.received_at));
            }
        }
    }
}
