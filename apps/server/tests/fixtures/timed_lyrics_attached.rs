use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    Capability, LyricsRequest, LyricsResponse, Manifest, PROTOCOL_VERSION, ProviderRecording,
    ProviderRecordingRef,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

pub const RECORDING: &str = "opaque:attached/lyrics?private-id";
pub const TOKEN: &str = "owned-attached-private-credential-canary";
pub const TITLE: &str = "Owned attached private \"recording\" canary";
pub const NAME: &str = "Owned attached lyric provider";

struct Fixture {
    lyrics_enabled: bool,
    reject_lyrics: AtomicBool,
    version: AtomicUsize,
    manifest: AtomicUsize,
    detail: AtomicUsize,
    sources: AtomicUsize,
    descriptors: AtomicUsize,
    lyrics: AtomicUsize,
    unexpected: AtomicUsize,
}

pub struct Provider {
    pub url: String,
    state: Arc<Fixture>,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Provider {
    pub async fn start() -> Self {
        Self::with_lyrics(true).await
    }

    pub async fn without_lyrics() -> Self {
        Self::with_lyrics(false).await
    }

    async fn with_lyrics(lyrics_enabled: bool) -> Self {
        let state = Arc::new(Fixture {
            lyrics_enabled,
            reject_lyrics: AtomicBool::new(false),
            version: AtomicUsize::new(1),
            manifest: AtomicUsize::new(0),
            detail: AtomicUsize::new(0),
            sources: AtomicUsize::new(0),
            descriptors: AtomicUsize::new(0),
            lyrics: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/attached-private/",
            listener.local_addr().unwrap()
        );
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let (shutdown, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = receiver.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            state,
            shutdown: Some(shutdown),
            task,
        }
    }

    pub fn reference(&self) -> ProviderRecordingRef {
        self.state.reference()
    }

    pub fn reject_lyrics(&self) {
        self.state.reject_lyrics.store(true, Ordering::SeqCst);
    }

    pub fn advance(&self) {
        assert_eq!(self.state.version.fetch_add(1, Ordering::SeqCst), 1);
    }

    pub fn assert_calls(&self, details: usize, sources: usize, lyrics: usize) {
        assert_eq!(self.state.manifest.load(Ordering::SeqCst), 1);
        assert_eq!(self.state.detail.load(Ordering::SeqCst), details);
        assert_eq!(self.state.sources.load(Ordering::SeqCst), sources);
        assert_eq!(self.state.descriptors.load(Ordering::SeqCst), lyrics);
        assert_eq!(self.state.lyrics.load(Ordering::SeqCst), lyrics);
        assert_eq!(self.state.unexpected.load(Ordering::SeqCst), 0);
    }

    pub async fn finish(mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(2), &mut self.task)
            .await
            .expect("The attached lyric provider must close within its shutdown deadline")
            .expect("The attached lyric provider must join shutdown");
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    fn reference(&self) -> ProviderRecordingRef {
        ProviderRecordingRef {
            id: RECORDING.into(),
            revision: Some(format!(
                "owned-attached:\"{}\"",
                self.version.load(Ordering::SeqCst)
            )),
        }
    }

    fn invalid(&self) -> Response {
        self.unexpected.fetch_add(1, Ordering::SeqCst);
        StatusCode::BAD_REQUEST.into_response()
    }
}

async fn exchange(State(state): State<Arc<Fixture>>, request: Request) -> Response {
    if request.uri().path().ends_with("/v1/health") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (parts, body) = request.into_parts();
    if parts.uri.query().is_some()
        || parts
            .headers
            .get("x-hito-plugin-protocol")
            .is_none_or(|value| value != "1")
        || parts
            .headers
            .get("authorization")
            .is_none_or(|value| value != format!("Bearer {TOKEN}").as_str())
    {
        return state.invalid();
    }
    let Ok(Ok(bytes)) = tokio::time::timeout(Duration::from_secs(2), to_bytes(body, 4096)).await
    else {
        return state.invalid();
    };
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        let Ok(value) = serde_json::from_slice(&bytes) else {
            return state.invalid();
        };
        value
    };
    let response = match (parts.method, parts.uri.path()) {
        (Method::GET, "/attached-private/v1/manifest") if body.is_null() => {
            state.manifest.fetch_add(1, Ordering::SeqCst);
            json!(Manifest {
                id: "org.hito.owned-attached-lyrics".into(),
                name: NAME.into(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: if state.lyrics_enabled {
                    [Capability::Sources, Capability::Lyrics].into()
                } else {
                    [Capability::Sources].into()
                },
                authorization: None,
                content_policy: None,
            })
        }
        (Method::POST, "/attached-private/v1/recordings/detail")
            if body == json!({"recording_id": RECORDING}) =>
        {
            state.detail.fetch_add(1, Ordering::SeqCst);
            json!(ProviderRecording {
                isrc: None,
                artist_urls: Vec::new(),
                artists: Vec::new(),
                id: RECORDING.into(),
                title: TITLE.into(),
                artist: "Owned attached performer".into(),
                duration_ms: Some(4000),
                release: None,
                disc_number: None,
                track_number: None,
                revision: state.reference().revision,
                attribution: None,
            })
        }
        (Method::POST, "/attached-private/v1/sources")
            if body == json!({"recording_id": RECORDING}) =>
        {
            state.sources.fetch_add(1, Ordering::SeqCst);
            json!({"items": []})
        }
        (Method::GET, "/attached-private/v1/lyrics/capabilities")
            if body.is_null() && state.lyrics_enabled =>
        {
            state.descriptors.fetch_add(1, Ordering::SeqCst);
            json!({"lyrics_version": 1})
        }
        (Method::POST, "/attached-private/v1/lyrics")
            if state.lyrics_enabled
                && body
                    == json!(LyricsRequest {
                        recording: state.reference()
                    }) =>
        {
            state.lyrics.fetch_add(1, Ordering::SeqCst);
            if state.reject_lyrics.load(Ordering::SeqCst) {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            json!(LyricsResponse {
                recording: state.reference(),
                lyrics: None
            })
        }
        _ => return state.invalid(),
    };
    Json(response).into_response()
}
