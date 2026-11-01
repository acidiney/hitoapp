use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION, ProviderRecording, ProviderSource, RecordingRequest,
    SourcePage,
};
use serde::Deserialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle};

pub const RECORDING_ID: &str = "owned-playback-recording";
pub const SOURCE_ID: &str = "owned-playback-source";

#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub manifests: usize,
    pub details: usize,
    pub sources: usize,
    pub audio: usize,
    pub unexpected: usize,
}

struct Requests {
    manifests: AtomicUsize,
    details: AtomicUsize,
    sources: AtomicUsize,
    audio: AtomicUsize,
    unexpected: AtomicUsize,
    original: Vec<u8>,
}

pub struct PlaybackServer {
    pub url: String,
    requests: Arc<Requests>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl PlaybackServer {
    pub async fn start(original: Vec<u8>) -> Self {
        assert!(original.len() <= 32_768);
        let requests = Arc::new(Requests {
            manifests: AtomicUsize::new(0),
            details: AtomicUsize::new(0),
            sources: AtomicUsize::new(0),
            audio: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            original,
        });
        let router = Router::new()
            .route("/prefix/v1/manifest", get(manifest))
            .route("/prefix/v1/recordings/detail", post(detail))
            .route("/prefix/v1/sources", post(sources))
            .route("/prefix/v1/audio", get(audio))
            .fallback(unexpected)
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        Self {
            url,
            requests,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn counts(&self) -> Counts {
        Counts {
            manifests: self.requests.manifests.load(Ordering::SeqCst),
            details: self.requests.details.load(Ordering::SeqCst),
            sources: self.requests.sources.load(Ordering::SeqCst),
            audio: self.requests.audio.load(Ordering::SeqCst),
            unexpected: self.requests.unexpected.load(Ordering::SeqCst),
        }
    }

    pub async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => matches!(result, Ok(true)),
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

impl Drop for PlaybackServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn manifest(State(requests): State<Arc<Requests>>) -> Json<Manifest> {
    requests.manifests.fetch_add(1, Ordering::SeqCst);
    Json(Manifest {
        id: "org.hito.artist-playback-fixture".into(),
        name: "Owned playback provider".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Sources].into(),
        authorization: None,
        content_policy: None,
    })
}

async fn detail(
    State(requests): State<Arc<Requests>>,
    Json(request): Json<RecordingRequest>,
) -> Response {
    requests.details.fetch_add(1, Ordering::SeqCst);
    if request.recording_id != RECORDING_ID {
        return reject(&requests);
    }
    Json(ProviderRecording {
        isrc: None,
        artist_urls: Vec::new(),
        artists: Vec::new(),
        id: RECORDING_ID.into(),
        title: "Owned playback recording".into(),
        artist: "Owned playback performer".into(),
        duration_ms: Some(1000),
        release: None,
        disc_number: None,
        track_number: None,
        revision: None,
        attribution: None,
    })
    .into_response()
}

async fn sources(
    State(requests): State<Arc<Requests>>,
    Json(request): Json<RecordingRequest>,
) -> Response {
    requests.sources.fetch_add(1, Ordering::SeqCst);
    if request.recording_id != RECORDING_ID {
        return reject(&requests);
    }
    Json(SourcePage {
        items: vec![ProviderSource {
            id: SOURCE_ID.into(),
            label: "Owned WAV".into(),
            ready: true,
        }],
    })
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioQuery {
    source_id: String,
}

async fn audio(
    State(requests): State<Arc<Requests>>,
    Query(request): Query<AudioQuery>,
) -> Response {
    requests.audio.fetch_add(1, Ordering::SeqCst);
    if request.source_id != SOURCE_ID {
        return reject(&requests);
    }
    ([("content-type", "audio/wav")], requests.original.clone()).into_response()
}

async fn unexpected(State(requests): State<Arc<Requests>>) -> StatusCode {
    requests.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::NOT_FOUND
}

fn reject(requests: &Requests) -> Response {
    requests.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::BAD_REQUEST.into_response()
}
