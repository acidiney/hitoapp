#[path = "state_inputs.rs"]
mod state_inputs;

use super::{fixtures, http, provider, scripted_media};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, ProviderRecording,
};
use hito_plugin_torbox::{Config, CuratedWavRecording};
use provider::reply::Reply;
use reqwest::{Method, StatusCode};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

const SERVICE_TOKEN: &str = "owned-private-torbox-uncertain-bearer";
const RECORDING: &str = "curated-uncertain-recording";
const SOURCE: &str = "curated-uncertain-source";

pub struct LostAcknowledgement {
    pub received: oneshot::Receiver<()>,
    pub peer_closed: oneshot::Receiver<()>,
    pub hold_acknowledgement: oneshot::Sender<()>,
}

pub struct Fixture {
    provider: provider::Fixture,
    media: scripted_media::ScriptedMedia,
    inputs: state_inputs::Inputs,
}

pub struct Owner {
    client: reqwest::Client,
    server: state_inputs::Server,
}

pub struct CapturedResponse {
    status: StatusCode,
    body: Vec<u8>,
}

impl Fixture {
    pub async fn start() -> (Self, LostAcknowledgement) {
        let (mut held, received, hold_acknowledgement) = Reply::paused(fixtures::acknowledgment());
        let peer_closed = held.observe_peer_close();
        let provider = provider::Fixture::responses(vec![
            Reply::json(fixtures::observed(true, true, "album/owned.wav")),
            held,
        ])
        .await;
        let media = scripted_media::ScriptedMedia::start(vec![scripted_media::Reply::full(
            fixtures::wave(),
        )])
        .await;
        (
            Self {
                provider,
                media,
                inputs: state_inputs::Inputs::new(),
            },
            LostAcknowledgement {
                received,
                peer_closed,
                hold_acknowledgement,
            },
        )
    }

    pub async fn open(&self) -> Owner {
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
                        isrc: None,
                        artist_urls: Vec::new(),
                        id: RECORDING.into(),
                        title: "owned-private-uncertain-title".into(),
                        artist: "owned-private-uncertain-artist".into(),
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
            .timeout(Duration::from_secs(16))
            .build()
            .unwrap();
        Owner { client, server }
    }

    pub fn media_requests(&self) -> usize {
        self.media.requests()
    }

    pub async fn close(self) -> (Vec<provider::Request>, usize) {
        (self.provider.close().await, self.media.close().await)
    }
}

impl Owner {
    pub async fn submit(&self, request: &PreparationRequest) -> CapturedResponse {
        self.post("v1/sources/prepare", request).await
    }

    pub async fn status(&self, receipt: &PreparationStatusRequest) -> CapturedResponse {
        self.post("v1/sources/preparation/status", receipt).await
    }

    pub async fn audio(&self) -> CapturedResponse {
        let (status, body) = http::json(
            &self.client,
            &self.server,
            Method::GET,
            &format!("v1/audio?source_id={SOURCE}"),
            super::COOKIE,
            Some(SERVICE_TOKEN),
            None,
        )
        .await;
        CapturedResponse { status, body }
    }

    async fn post(&self, path: &str, body: &impl serde::Serialize) -> CapturedResponse {
        let (status, body) = http::json(
            &self.client,
            &self.server,
            Method::POST,
            path,
            super::COOKIE,
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(body).unwrap()),
        )
        .await;
        CapturedResponse { status, body }
    }

    pub async fn close(self) {
        let Self { client, server } = self;
        drop(client);
        server
            .close()
            .await
            .expect("the real HTTP and SQLite owners must join shutdown");
    }
}

impl CapturedResponse {
    pub fn accepted_receipt(
        &self,
        request: &PreparationRequest,
    ) -> Option<PreparationStatusRequest> {
        if self.status != StatusCode::OK {
            return None;
        }
        let response: PreparationSubmitResponse = serde_json::from_slice(&self.body).ok()?;
        if response.validate(request).is_err() {
            return None;
        }
        match response.submission {
            PreparationSubmission::Accepted { handle, .. } => Some(PreparationStatusRequest {
                operation_id: request.operation_id,
                recording_id: request.recording_id.clone(),
                source_id: request.source_id.clone(),
                handle,
            }),
            _ => None,
        }
    }

    pub fn assert_unknown_submission(
        &self,
        request: &PreparationRequest,
        receipt: &PreparationStatusRequest,
    ) {
        assert_eq!(self.status, StatusCode::OK);
        let response: PreparationSubmitResponse = serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| {
                panic!("the public preparation response must retain its typed wire shape")
            });
        assert!(response.validate(request).is_ok());
        assert!(
            matches!(response.submission, PreparationSubmission::Accepted {
            handle, status: PreparationStatus::SubmissionUnknown {},
        } if handle == receipt.handle)
        );
    }

    pub fn assert_unknown_status(&self, receipt: &PreparationStatusRequest) {
        assert_eq!(self.status, StatusCode::OK);
        let response: PreparationStatusResponse = serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| {
                panic!("the public status response must retain its typed wire shape")
            });
        assert!(response.validate(receipt).is_ok());
        assert!(matches!(
            response.status,
            PreparationStatus::SubmissionUnknown {}
        ));
    }

    pub fn assert_closed_error(&self, status: StatusCode, message: &str) {
        assert_eq!(self.status, status);
        let body: serde_json::Value = serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("the failed public response must remain closed JSON"));
        let expected = serde_json::json!({"error":message});
        assert!(body == expected);
    }
}

pub fn request() -> PreparationRequest {
    PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
    }
}

pub fn assert_requests(requests: &[provider::Request]) {
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
            .count(),
        1
    );
    for request in requests {
        assert!(
            request.server_name == "api.torbox.app"
                && request.method == "GET"
                && request.body.is_empty()
        );
        for header in [
            "cookie",
            "referer",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(header).is_none());
        }
    }
    let observed = &requests[0];
    let expected_authorization = format!("Bearer {}", fixtures::TOKEN);
    let expected_observation = "/v1/api/torrents/mylist?id=7123&bypass_cache=true";
    assert!(observed.target == expected_observation);
    assert!(observed.header("authorization") == Some(expected_authorization.as_str()));
    let expected_mint = "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false";
    assert!(requests[1].target == expected_mint && requests[1].header("authorization").is_none());
}
