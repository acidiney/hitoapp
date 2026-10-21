#[path = "state_inputs.rs"]
#[allow(dead_code)]
mod state_inputs;

use super::{
    catalog_entries, entries, fixtures, http, provider, scripted_media, uncertain_provider,
};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse,
};
use hito_plugin_torbox::{Config, CuratedWavRecording, Service};
use reqwest::{Method, StatusCode};
use std::{
    net::SocketAddr,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;
use tracing::{
    Subscriber,
    field::{Field, Visit},
    instrument::WithSubscriber,
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use uncertain_provider::reply::Reply;

const SERVICE_TOKEN: &str = "owned-private-torbox-capacity-uncertain-bearer";
pub type Response = (StatusCode, Vec<u8>);

pub struct Saved {
    pub request: PreparationRequest,
    receipt: PreparationStatusRequest,
}

pub struct LostAcknowledgement {
    pub received: oneshot::Receiver<()>,
    pub peer_closed: oneshot::Receiver<()>,
    pub hold_acknowledgement: oneshot::Sender<()>,
}

pub struct Fixture {
    uncertain: Option<uncertain_provider::Fixture>,
    provider: Option<provider::Fixture>,
    media: scripted_media::ScriptedMedia,
    inputs: state_inputs::Inputs,
    events: CapacityEvents,
}

pub struct Owner {
    client: reqwest::Client,
    server: http::Server,
    service: Service,
}

impl Fixture {
    pub async fn start() -> (Self, LostAcknowledgement) {
        let (mut held, received, hold_acknowledgement) = Reply::paused(fixtures::acknowledgment());
        let peer_closed = held.observe_peer_close();
        let uncertain =
            uncertain_provider::Fixture::responses(vec![Reply::json(unknown_observation()), held])
                .await;
        let media = scripted_media::ScriptedMedia::start(vec![scripted_media::Reply::full(
            fixtures::wave(),
        )])
        .await;
        (
            Self {
                uncertain: Some(uncertain),
                provider: None,
                media,
                inputs: state_inputs::Inputs::new(),
                events: CapacityEvents::default(),
            },
            LostAcknowledgement {
                received,
                peer_closed,
                hold_acknowledgement,
            },
        )
    }

    pub async fn open_uncertain(&self) -> Owner {
        let provider = self
            .uncertain
            .as_ref()
            .expect("the original provider must still be owned");
        self.open_with(
            provider.ca_der(),
            provider.address(),
            vec![unknown_recording()],
        )
        .await
    }

    pub async fn start_capacity(&mut self) -> (usize, usize) {
        let requests = self
            .uncertain
            .take()
            .expect("the original provider must join exactly once")
            .close()
            .await;
        let counts = (
            requests.len(),
            requests
                .iter()
                .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
                .count(),
        );
        assert_uncertain_requests(&requests);
        assert_eq!(self.media.requests(), 0);
        self.provider = Some(provider::Fixture::start().await);
        counts
    }

    pub async fn open(&self, number: usize) -> Owner {
        let provider = self
            .provider
            .as_ref()
            .expect("the capacity provider must replace the joined original provider");
        let indices = batch(number);
        let mut catalog = entries::catalog(0);
        catalog.truncate(1 + indices.len());
        for (entry, index) in catalog[1..].iter_mut().zip(indices) {
            let request = entries::request(index);
            entry.recording.id = request.recording_id;
            entry.source_id = request.source_id;
            entry.binding = entries::binding(index);
        }
        catalog.push(unknown_recording());
        assert!(catalog.len() <= 128);
        self.open_with(provider.ca_der(), provider.address(), catalog)
            .await
    }

    async fn open_with(
        &self,
        ca: &[u8],
        address: SocketAddr,
        recordings: Vec<CuratedWavRecording>,
    ) -> Owner {
        let dispatch =
            tracing::Dispatch::new(tracing_subscriber::registry().with(self.events.clone()));
        let service = Service::open(Config {
            state: self.inputs.next(),
            service_bearer: SERVICE_TOKEN.into(),
            client: TorBoxClient::with_test_tls_media(
                fixtures::TOKEN.into(),
                ca,
                address,
                self.media.ca_der(),
                self.media.address(),
            )
            .unwrap(),
            media_origins: MediaAllowlist::new(vec![format!("https://{}", fixtures::MEDIA_HOST)])
                .unwrap(),
            recordings,
        })
        .with_subscriber(dispatch.clone())
        .await
        .expect("each real journal owner must preserve the original Unknown target");
        let server = http::Server::start(service.router(), dispatch).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(16))
            .build()
            .unwrap();
        Owner {
            client,
            server,
            service,
        }
    }

    pub fn provider_counts(&self) -> provider::Counts {
        self.provider
            .as_ref()
            .map_or_else(provider::Counts::default, provider::Fixture::counts)
    }

    pub fn media_requests(&self) -> usize {
        self.media.requests()
    }

    pub async fn close(self) -> (provider::Counts, usize, usize) {
        assert!(self.uncertain.is_none());
        let provider = self
            .provider
            .expect("the capacity provider must remain owned through shutdown");
        let counts = provider.counts();
        let _ = provider.close().await;
        (
            counts,
            self.media.close().await,
            self.events.0.load(Ordering::SeqCst),
        )
    }
}

impl Owner {
    pub async fn prepare_ready_control(&self) {
        let request = entries::ready_request();
        let (status, body) = self.submit(&request).await;
        assert_eq!(status, StatusCode::OK);
        let response: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
        assert!(response.validate(&request).is_ok());
        assert!(matches!(
            response.submission,
            PreparationSubmission::Accepted {
                status: PreparationStatus::Ready {},
                ..
            }
        ));
    }

    pub async fn submit(&self, request: &PreparationRequest) -> Response {
        self.post("v1/sources/prepare", request).await
    }
    pub async fn status(&self, saved: &Saved) -> Response {
        self.post("v1/sources/preparation/status", &saved.receipt)
            .await
    }

    pub async fn audio(&self) -> Response {
        http::json(
            &self.client,
            &self.server,
            Method::GET,
            &format!("v1/audio?source_id={}", catalog_entries::ADDED_SOURCE),
            Some(SERVICE_TOKEN),
            None,
        )
        .await
    }

    async fn post(&self, path: &str, body: &impl serde::Serialize) -> Response {
        http::json(
            &self.client,
            &self.server,
            Method::POST,
            path,
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(body).unwrap()),
        )
        .await
    }

    pub async fn close(self) -> Result<(), &'static str> {
        let Self {
            client,
            server,
            service,
        } = self;
        drop(client);
        let http = server.close().await;
        let state = service.close().await;
        http?;
        state.map_err(|_| "the real Unknown SQLite owner did not join shutdown")
    }
}

pub fn batch(number: usize) -> Range<usize> {
    let start = number
        .checked_mul(126)
        .expect("the Unknown capacity catalog batch must be bounded");
    assert!(start < 1022);
    start..(start + 126).min(1022)
}

pub fn unknown_request() -> PreparationRequest {
    PreparationRequest {
        operation_id: uuid::Uuid::new_v4(),
        recording_id: catalog_entries::ADDED_RECORDING.into(),
        source_id: catalog_entries::ADDED_SOURCE.into(),
    }
}

fn unknown_recording() -> CuratedWavRecording {
    catalog_entries::original(true)
        .into_iter()
        .find(|entry| entry.source_id == catalog_entries::ADDED_SOURCE)
        .expect("the existing unrelated control must retain its exact full binding")
}

fn unknown_observation() -> Vec<u8> {
    let binding = catalog_entries::control_binding();
    serde_json::to_vec(&serde_json::json!({
        "success":true, "error":null, "detail":"owned-private-control-observation",
        "data":{"id":binding.torrent_id,"hash":binding.info_hash,"size":binding.torrent_bytes,
            "download_finished":true,"download_present":true,"download_state":"completed",
            "name":"owned-private-control-title","files":[
                {"id":binding.file_id,"name":binding.file_name,"size":binding.file_bytes}
            ]}
    }))
    .unwrap()
}

pub fn saved(request: &PreparationRequest, (status, body): &Response) -> Saved {
    assert_eq!(*status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(body).unwrap();
    assert!(response.validate(request).is_ok());
    let PreparationSubmission::Accepted { handle, .. } = response.submission else {
        panic!("the in-flight Unknown operation must return its actual public handle")
    };
    Saved {
        request: request.clone(),
        receipt: PreparationStatusRequest {
            operation_id: request.operation_id,
            recording_id: request.recording_id.clone(),
            source_id: request.source_id.clone(),
            handle,
        },
    }
}

pub fn assert_unknown_submission((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.validate(&saved.request).is_ok());
    assert!(
        matches!(response.submission, PreparationSubmission::Accepted {
        handle, status: PreparationStatus::SubmissionUnknown {},
    } if handle == saved.receipt.handle)
    );
}

pub fn assert_unknown_status((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationStatusResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.validate(&saved.receipt).is_ok());
    assert!(matches!(
        response.status,
        PreparationStatus::SubmissionUnknown {}
    ));
}

pub fn assert_preparing((status, body): Response, request: &PreparationRequest) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.validate(request).is_ok());
    assert!(matches!(
        response.submission,
        PreparationSubmission::Accepted {
            status: PreparationStatus::Preparing { .. },
            ..
        }
    ));
}

fn assert_closed_error((status, body): Response, expected: StatusCode, message: &str) {
    assert_eq!(status, expected);
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(body == serde_json::json!({"error":message}));
}

pub fn assert_closed_capacity(response: Response) {
    assert_closed_error(
        response,
        StatusCode::TOO_MANY_REQUESTS,
        "The service is busy",
    );
}
pub fn assert_unavailable_audio(response: Response) {
    assert_closed_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "The source is unavailable",
    );
}
pub fn assert_conflict(response: Response) {
    assert_closed_error(
        response,
        StatusCode::CONFLICT,
        "The operation belongs to another source",
    );
}

pub fn closed_error_kind((_, body): &Response) -> &'static str {
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(body) else {
        return "invalid_json";
    };
    if body == serde_json::json!({"error":"The service is busy"}) {
        "busy"
    } else if body == serde_json::json!({"error":"The source is unavailable"}) {
        "unavailable"
    } else {
        "unexpected_closed_error"
    }
}

fn assert_uncertain_requests(requests: &[uncertain_provider::Request]) {
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
            .count(),
        1
    );
    let expected_authorization = format!("Bearer {}", fixtures::TOKEN);
    let observation = "/v1/api/torrents/mylist?id=8124&bypass_cache=true";
    let mint = "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=8124&file_id=19&zip_link=false&redirect=false&append_name=false";
    for (index, request) in requests.iter().enumerate() {
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
        if index == 0 {
            assert!(
                request.target == observation
                    && request.header("authorization") == Some(expected_authorization.as_str())
            );
        } else {
            assert!(request.target == mint && request.header("authorization").is_none());
        }
    }
}

#[derive(Clone, Default)]
struct CapacityEvents(Arc<AtomicUsize>);

impl<S: Subscriber> Layer<S> for CapacityEvents {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        #[derive(Default)]
        struct Fields {
            admission: bool,
            capacity: bool,
            database: bool,
            database_outcome: bool,
            code: Option<u64>,
        }
        impl Visit for Fields {
            fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &Field, value: &str) {
                match field.name() {
                    "event" => {
                        self.admission = value == "torbox_journal_admission_rejected";
                        self.database = value == "torbox_journal_database_rejected";
                    }
                    "outcome" => {
                        self.capacity = value == "capacity";
                        self.database_outcome = value == "database_error";
                    }
                    _ => {}
                }
            }
            fn record_u64(&mut self, field: &Field, value: u64) {
                if field.name() == "sqlite_code" {
                    self.code = Some(value);
                }
            }
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.database
            && fields.database_outcome
            && let Some(code) = fields.code
        {
            eprintln!("unknown_capacity_sqlite_error code={code}");
        }
        if fields.admission {
            assert!(
                fields.capacity && fields.code == Some(13),
                "capacity evidence must contain actual numeric SQLITE_FULL"
            );
            assert!(self.0.fetch_add(1, Ordering::SeqCst) < 3);
        }
    }
}
