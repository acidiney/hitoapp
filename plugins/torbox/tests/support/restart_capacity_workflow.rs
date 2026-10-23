#[path = "state_inputs.rs"]
#[allow(dead_code)]
mod state_inputs;

use super::{entries, fixtures, http, provider, scripted_media};
use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse,
};
use hito_plugin_torbox::{Config, Service};
use reqwest::{Method, StatusCode};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tracing::{
    Subscriber,
    field::{Field, Visit},
    instrument::WithSubscriber,
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

const SERVICE_TOKEN: &str = "owned-private-torbox-capacity-bearer";
pub type Response = (StatusCode, Vec<u8>);

pub struct Saved {
    pub request: PreparationRequest,
    receipt: PreparationStatusRequest,
}

pub struct Fixture {
    provider: provider::Fixture,
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
    pub async fn start() -> Self {
        Self {
            provider: provider::Fixture::start().await,
            media: scripted_media::ScriptedMedia::start(vec![
                scripted_media::Reply::full(fixtures::wave()),
                scripted_media::Reply::full(fixtures::wave()),
            ])
            .await,
            inputs: state_inputs::Inputs::new(),
            events: CapacityEvents::default(),
        }
    }

    pub async fn open(&self, batch: usize) -> Owner {
        let dispatch =
            tracing::Dispatch::new(tracing_subscriber::registry().with(self.events.clone()));
        let service = Service::open(Config {
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
            media_origins: MediaAllowlist::new(vec![format!("https://{}", fixtures::MEDIA_HOST)])
                .unwrap(),
            recordings: entries::catalog(batch),
        })
        .with_subscriber(dispatch.clone())
        .await
        .expect("the real journal owner must accept the bounded configured catalog");
        let server = http::Server::start(service.router(), dispatch).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap();
        Owner {
            client,
            server,
            service,
        }
    }

    pub fn provider_counts(&self) -> provider::Counts {
        self.provider.counts()
    }

    pub async fn close(self) -> (provider::Transcript, usize, usize) {
        (
            self.provider.close().await,
            self.media.close().await,
            self.events.0.load(Ordering::SeqCst),
        )
    }
}

impl Owner {
    pub async fn prepare_ready(&self) -> Saved {
        let request = entries::ready_request();
        let (status, body) = self.submit(&request).await;
        assert_eq!(status, StatusCode::OK);
        let response: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
        response.validate(&request).unwrap();
        let PreparationSubmission::Accepted {
            handle,
            status: PreparationStatus::Ready {},
        } = response.submission
        else {
            panic!("the original exact source must be Ready before filling the private journal")
        };
        let receipt = PreparationStatusRequest {
            operation_id: request.operation_id,
            recording_id: request.recording_id.clone(),
            source_id: request.source_id.clone(),
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

    pub async fn audio(&self) -> reqwest::Response {
        self.client
            .get(
                self.server
                    .base
                    .join(&format!("v1/audio?source_id={}", entries::SOURCE))
                    .unwrap(),
            )
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", super::COOKIE)
            .send()
            .await
            .expect("the retained audio HTTP request must finish")
    }

    async fn post(&self, path: &str, request: &impl serde::Serialize) -> Response {
        http::json(
            &self.client,
            &self.server,
            Method::POST,
            path,
            super::COOKIE,
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(request).unwrap()),
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
        state.map_err(|_| "the real SQLite owner did not close cleanly")
    }
}

pub fn assert_preparing((status, body): Response, request: &PreparationRequest) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    response.validate(request).unwrap();
    assert!(matches!(
        response.submission,
        PreparationSubmission::Accepted {
            status: PreparationStatus::Preparing { .. },
            ..
        }
    ));
}

pub fn assert_ready_status((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationStatusResponse = serde_json::from_slice(&body).unwrap();
    response.validate(&saved.receipt).unwrap();
    assert!(matches!(response.status, PreparationStatus::Ready {}));
}

pub fn assert_ready_replay((status, body): Response, saved: &Saved) {
    assert_eq!(status, StatusCode::OK);
    let response: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
    response.validate(&saved.request).unwrap();
    assert!(
        matches!(response.submission, PreparationSubmission::Accepted {
        handle, status: PreparationStatus::Ready {},
    } if handle == saved.receipt.handle)
    );
}

pub fn assert_closed_capacity((status, body): Response) {
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(body == serde_json::json!({"error":"The service is busy"}));
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

pub async fn complete(mut response: reqwest::Response) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert!(response.headers()["content-length"] == fixtures::WAV_BYTES.to_string());
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(chunk.len() <= (fixtures::WAV_BYTES as usize).saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk);
    }
    assert!(bytes == fixtures::wave());
    fixtures::assert_pcm(bytes);
}

#[derive(Clone, Default)]
struct CapacityEvents(Arc<AtomicUsize>);

impl<S: Subscriber> Layer<S> for CapacityEvents {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        #[derive(Default)]
        struct Fields {
            event: bool,
            capacity: bool,
            database_error: bool,
            database_outcome: bool,
            code: Option<u64>,
        }
        impl Visit for Fields {
            fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &Field, value: &str) {
                match field.name() {
                    "event" => {
                        self.event = value == "torbox_journal_admission_rejected";
                        self.database_error = value == "torbox_journal_database_rejected";
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
        if fields.database_error
            && fields.database_outcome
            && let Some(code) = fields.code
        {
            eprintln!("capacity_sqlite_error code={code}");
        }
        if fields.event {
            assert!(
                fields.capacity && fields.code == Some(13),
                "capacity evidence must contain the actual closed SQLite result code"
            );
            assert!(self.0.fetch_add(1, Ordering::SeqCst) < 3);
        }
    }
}
