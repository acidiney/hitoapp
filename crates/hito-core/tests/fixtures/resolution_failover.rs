use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_core::{Actor, InstallPlugin, OfferPluginService, PluginInstallation, Services};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION, ProviderRecording, ProviderSource, RecordingMetadata,
    RecordingRequest, SearchPage, SearchRequest, SourcePage,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

pub const FAILED: &str = "owned-failed-candidate";
pub const READY: &str = "owned-ready-candidate";
const SOURCE: &str = "owned-ready-audio";

#[derive(Clone, Copy)]
pub enum Plan {
    Empty,
    SearchUnavailable,
    FailedThenReady,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Manifest,
    Search,
    FailedDetail,
    FailedSources,
    ReadyDetail,
    ReadySources,
    Metadata,
    Audio,
    Unexpected,
}

struct Fixture {
    plan: Plan,
    original: Vec<u8>,
    events: Mutex<Vec<Event>>,
}

pub struct Provider {
    url: String,
    state: Arc<Fixture>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl Provider {
    pub async fn start(plan: Plan, original: Vec<u8>) -> Self {
        for id in [FAILED, READY] {
            assert!(recording(id).validate().is_ok());
        }
        assert!(metadata().validate().is_ok());
        let state = Arc::new(Fixture {
            plan,
            original,
            events: Mutex::new(Vec::new()),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let app = Router::new().fallback(respond).with_state(state.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            url,
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn connect(&self, services: &Services, actor: &Actor) -> PluginInstallation {
        let service = services
            .plugins
            .offer_service(
                actor,
                OfferPluginService {
                    name: "Owned resolution failover provider".into(),
                    base_url: self.url.clone(),
                },
            )
            .await
            .unwrap_or_else(|_| panic!("the owned failover service must be offered"));
        services
            .plugins
            .install(
                actor,
                InstallPlugin {
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                    accept_manifest_digest: None,
                },
            )
            .await
            .unwrap_or_else(|_| panic!("the owned failover service must connect"))
    }

    pub async fn events(&self) -> Vec<Event> {
        self.state.events.lock().await.clone()
    }

    pub async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
            Ok(result) => matches!(result, Ok(true)),
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Fixture {
    async fn record(&self, event: Event) {
        let mut events = self.events.lock().await;
        assert!(
            events.len() < 32,
            "the owned protocol fixture must stay within its request bound"
        );
        events.push(event);
    }
}

async fn respond(State(state): State<Arc<Fixture>>, request: Request) -> Response {
    if request
        .headers()
        .get("x-hito-plugin-protocol")
        .is_none_or(|value| value != "1")
    {
        state.record(Event::Unexpected).await;
        return StatusCode::BAD_REQUEST.into_response();
    }
    let path = request.uri().path().to_owned();
    if request.method() == Method::GET && path == "/v1/manifest" {
        state.record(Event::Manifest).await;
        return Json(Manifest {
            id: "org.hito.failover-fixture".into(),
            name: "Owned failover provider".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [
                Capability::Search,
                Capability::Sources,
                Capability::Metadata,
            ]
            .into(),
            authorization: None,
            content_policy: None,
        })
        .into_response();
    }
    if request.method() == Method::GET
        && path == "/v1/audio"
        && request.uri().query() == Some("source_id=owned-ready-audio")
    {
        state.record(Event::Audio).await;
        return (
            [("content-type", "audio/wav"), ("accept-ranges", "bytes")],
            state.original.clone(),
        )
            .into_response();
    }
    if request.method() != Method::POST {
        state.record(Event::Unexpected).await;
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(body) = to_bytes(request.into_body(), 4096).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if path == "/v1/search" {
        let Ok(search) = serde_json::from_slice::<SearchRequest>(&body) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if search.validate().is_err()
            || search.query != "Owned performer Owned tune"
            || search.cursor.is_some()
            || search.limit != 20
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        state.record(Event::Search).await;
        return match state.plan {
            Plan::Empty => Json(SearchPage {
                items: Vec::new(),
                next_cursor: None,
            })
            .into_response(),
            Plan::SearchUnavailable => unavailable(),
            Plan::FailedThenReady => Json(SearchPage {
                items: vec![recording(FAILED), recording(READY)],
                next_cursor: None,
            })
            .into_response(),
        };
    }
    let Ok(recording_request) = serde_json::from_slice::<RecordingRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if recording_request.validate().is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match (path.as_str(), recording_request.recording_id.as_str()) {
        ("/v1/recordings/detail", FAILED) => {
            state.record(Event::FailedDetail).await;
            Json(recording(FAILED)).into_response()
        }
        ("/v1/recordings/detail", READY) => {
            state.record(Event::ReadyDetail).await;
            Json(recording(READY)).into_response()
        }
        ("/v1/sources", FAILED) => {
            state.record(Event::FailedSources).await;
            unavailable()
        }
        ("/v1/sources", READY) => {
            state.record(Event::ReadySources).await;
            Json(SourcePage {
                items: vec![ProviderSource {
                    id: SOURCE.into(),
                    label: "Owned ready audio".into(),
                    ready: true,
                }],
            })
            .into_response()
        }
        ("/v1/recordings/metadata", READY) => {
            state.record(Event::Metadata).await;
            Json(metadata()).into_response()
        }
        _ => {
            state.record(Event::Unexpected).await;
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Owned provider unavailable"})),
    )
        .into_response()
}

fn recording(id: &str) -> ProviderRecording {
    serde_json::from_value(
        json!({"id": id, "title": "Owned tune", "artist": "Owned performer", "duration_ms": 1000}),
    )
    .unwrap_or_else(|_| panic!("the owned candidate must use the actual SDK wire type"))
}

fn metadata() -> RecordingMetadata {
    RecordingMetadata {
        recording: recording(READY),
        musical_title: Some("Owned tune".into()),
        album_title: Some("Owned studio album".into()),
        source_ids: Some(vec![SOURCE.into()]),
    }
}
