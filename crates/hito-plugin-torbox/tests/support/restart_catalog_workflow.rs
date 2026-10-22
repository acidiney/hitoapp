use super::{entries, fixtures, http, provider, scripted_media, state_inputs};
use entries::{ADDED_RECORDING, RECORDING, SOURCE};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, ProviderRecording,
};
use hito_plugin_torbox::{Config, CuratedWavRecording, InitError, Service, StateError};
use provider::reply::Reply;
use reqwest::{Method, StatusCode};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

const SERVICE_TOKEN: &str = "owned-private-torbox-catalog-bearer";

pub struct Fixture {
    provider: provider::Fixture,
    media: scripted_media::ScriptedMedia,
    inputs: state_inputs::Inputs,
}

pub struct Owner {
    client: reqwest::Client,
    server: http::Server,
    service: Service,
}

pub struct Saved {
    pub request: PreparationRequest,
    pub receipt: PreparationStatusRequest,
}

pub type Response = (StatusCode, Vec<u8>);

impl Fixture {
    pub fn state_directory(&self) -> PathBuf {
        self.inputs.directory.path().join("journal")
    }

    pub async fn start() -> Self {
        Self::scripted(
            vec![
                Reply::json(fixtures::observed(true, true, "album/owned.wav")),
                Reply::json(fixtures::acknowledgment()),
                Reply::json(fixtures::observed(true, true, "album/owned.wav")),
            ],
            1,
        )
        .await
    }

    pub async fn start_removal() -> Self {
        Self::scripted(
            vec![
                Reply::json(fixtures::observed(true, true, "album/owned.wav")),
                Reply::json(fixtures::acknowledgment()),
                Reply::json(fixtures::observed(true, true, "album/owned.wav")),
                Reply::json(fixtures::observed(true, true, "album/owned.wav")),
            ],
            2,
        )
        .await
    }

    pub(super) async fn scripted(replies: Vec<Reply>, audio_gets: usize) -> Self {
        let provider = provider::Fixture::responses(replies).await;
        let media = scripted_media::ScriptedMedia::start(
            (0..audio_gets)
                .map(|_| scripted_media::Reply::full(fixtures::wave()))
                .collect(),
        )
        .await;
        Self {
            provider,
            media,
            inputs: state_inputs::Inputs::new(),
        }
    }

    pub async fn open(&self, include_unrelated: bool) -> Result<Owner, InitError> {
        self.open_with(
            entries::original(include_unrelated),
            Duration::from_secs(4),
            None,
        )
        .await
    }

    pub async fn open_catalog(&self, selection: entries::Evolution) -> Result<Owner, InitError> {
        self.open_with(selection.recordings(), Duration::from_secs(16), None)
            .await
    }

    pub async fn open_using_key(&self, key_file: PathBuf) -> Result<Owner, InitError> {
        self.open_with(
            entries::original(false),
            Duration::from_secs(4),
            Some(key_file),
        )
        .await
    }

    pub(super) async fn open_with(
        &self,
        recordings: Vec<CuratedWavRecording>,
        deadline: Duration,
        key_file: Option<PathBuf>,
    ) -> Result<Owner, InitError> {
        let mut state = self.inputs.next();
        if let Some(key_file) = key_file {
            state.key_file = key_file;
        }
        let service = Service::open(Config {
            state,
            service_bearer: SERVICE_TOKEN.into(),
            client: TorBoxClient::with_test_tls_media(
                fixtures::TOKEN.into(),
                self.provider.ca_der(),
                self.provider.address(),
                self.media.ca_der(),
                self.media.address(),
            )
            .unwrap(),
            media_origins: MediaAllowlist::new(vec![format!("https://{}", fixtures::MEDIA_HOST)])
                .unwrap(),
            recordings,
        })
        .await?;
        let server = http::Server::start(service.router(), tracing::Dispatch::none()).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .timeout(deadline)
            .build()
            .unwrap();
        Ok(Owner {
            client,
            server,
            service,
        })
    }

    pub fn media_requests(&self) -> usize {
        self.media.requests()
    }

    pub async fn close(self) -> (Vec<provider::Request>, usize) {
        (self.provider.close().await, self.media.close().await)
    }
}

impl Owner {
    pub async fn prepare(&self) -> Saved {
        self.prepare_target(RECORDING, SOURCE).await
    }

    pub async fn prepare_target(&self, recording: &str, source: &str) -> Saved {
        let request = request(recording, source);
        let (status, body) = self.submit(&request).await;
        assert_eq!(status, StatusCode::OK);
        let response: PreparationSubmitResponse = serde_json::from_slice(&body)
            .unwrap_or_else(|_| panic!("initial preparation must return its typed wire response"));
        assert!(response.validate(&request).is_ok());
        let PreparationSubmission::Accepted {
            handle,
            status: PreparationStatus::Ready {},
        } = response.submission
        else {
            panic!("the original exact selection must reach Ready before catalog evolution")
        };
        let receipt = PreparationStatusRequest {
            operation_id: request.operation_id,
            recording_id: recording.into(),
            source_id: source.into(),
            handle,
        };
        Saved { request, receipt }
    }

    pub async fn submit(&self, request: &PreparationRequest) -> Response {
        self.post("v1/sources/prepare", request).await
    }

    pub async fn status(&self, saved: &Saved) -> Response {
        self.post("v1/sources/preparation/status", &saved.receipt)
            .await
    }

    pub async fn replay(&self, saved: &Saved) -> Response {
        self.submit(&saved.request).await
    }

    pub async fn added_detail(&self) -> Response {
        self.post(
            "v1/recordings/detail",
            &serde_json::json!({"recording_id":ADDED_RECORDING}),
        )
        .await
    }

    pub async fn sources(&self, recording: &str) -> Response {
        self.post("v1/sources", &serde_json::json!({"recording_id":recording}))
            .await
    }

    pub async fn audio(&self) -> reqwest::Response {
        self.audio_source(SOURCE).await
    }

    pub async fn audio_source(&self, source: &str) -> reqwest::Response {
        self.client
            .get(
                self.server
                    .base
                    .join(&format!("v1/audio?source_id={source}"))
                    .unwrap(),
            )
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", super::COOKIE)
            .send()
            .await
            .expect("the retained source audio HTTP request must complete")
    }

    async fn post(&self, path: &str, body: &impl serde::Serialize) -> Response {
        http::json(
            &self.client,
            &self.server,
            Method::POST,
            path,
            super::COOKIE,
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(body).unwrap()),
        )
        .await
    }

    pub async fn close(self) {
        self.close_result()
            .await
            .expect("the real SQLite owner must join before replacement");
    }

    pub async fn close_result(self) -> Result<(), StateError> {
        let Self {
            client,
            server,
            service,
        } = self;
        drop(client);
        let http = server.close().await;
        let state = service.close().await;
        http.expect("the real HTTP owner must join before replacement");
        state
    }
}

pub fn request(recording: &str, source: &str) -> PreparationRequest {
    PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: recording.into(),
        source_id: source.into(),
    }
}

pub fn saved(request: &PreparationRequest, (status, body): &Response) -> Option<Saved> {
    if *status != StatusCode::OK {
        return None;
    }
    let response: PreparationSubmitResponse = serde_json::from_slice(body).ok()?;
    if response.validate(request).is_err() {
        return None;
    }
    let PreparationSubmission::Accepted { handle, .. } = response.submission else {
        return None;
    };
    Some(Saved {
        request: request.clone(),
        receipt: PreparationStatusRequest {
            operation_id: request.operation_id,
            recording_id: request.recording_id.clone(),
            source_id: request.source_id.clone(),
            handle,
        },
    })
}

pub fn assert_added_detail((status, body): Response) {
    assert_eq!(status, StatusCode::OK);
    let recording: ProviderRecording = serde_json::from_slice(&body).unwrap_or_else(|_| {
        panic!("the new configured catalog entry must have a typed detail response")
    });
    assert!(recording.validate().is_ok() && recording.id == ADDED_RECORDING);
}

pub fn assert_ready_status((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationStatusResponse = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("retained preparation status must have its typed wire shape"));
    assert!(response.validate(&saved.receipt).is_ok());
    assert!(matches!(response.status, PreparationStatus::Ready {}));
}

pub fn assert_ready_replay((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("retained preparation replay must have its typed wire shape"));
    assert!(response.validate(&saved.request).is_ok());
    assert!(
        matches!(response.submission, PreparationSubmission::Accepted {
        handle, status: PreparationStatus::Ready {},
    } if handle == saved.receipt.handle)
    );
}

pub fn assert_requests(
    requests: &[provider::Request],
    get_window: Option<(tokio::time::Instant, tokio::time::Instant)>,
) {
    assert_eq!(requests.len(), if get_window.is_some() { 3 } else { 2 });
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
            .count(),
        1
    );
    let expected_observation = "/v1/api/torrents/mylist?id=7123&bypass_cache=true";
    let expected_mint = "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false";
    let expected_authorization = format!("Bearer {}", fixtures::TOKEN);
    for (index, request) in requests.iter().enumerate() {
        assert!(
            request.method == "GET"
                && request.server_name == "api.torbox.app"
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
        if index == 1 {
            assert!(request.target == expected_mint && request.header("authorization").is_none());
        } else {
            assert!(
                request.target == expected_observation
                    && request.header("authorization") == Some(expected_authorization.as_str())
            );
        }
    }
    if let Some((started, finished)) = get_window {
        assert!((started..=finished).contains(&requests[2].received_at));
    }
}
