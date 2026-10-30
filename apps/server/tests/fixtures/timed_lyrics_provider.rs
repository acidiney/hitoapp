use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use hito_plugin_sdk::{
    Capability, LyricsRequest, LyricsResponse, Manifest, PROTOCOL_VERSION, PlaylistAttribution,
    PlaylistPage, ProviderPlaylist, ProviderPlaylistEntry, ProviderPlaylistItem, ProviderRecording,
    ProviderRecordingRef, TimedLyricLine, TimedLyrics,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, oneshot};

#[path = "timed_lyrics_body.rs"]
mod body;
pub use body::HeldBody;

pub const PROVIDER_ID: &str = "opaque:lyrics/owned?private-reference";
pub const TOKEN: &str = "owned-lyrics-credential-canary";
pub const TITLE: &str = "Owned private \"lyric\" recording canary";
pub const LINE: &str = "Owned private \"lyric\" text canary";
pub const RECORDING_REVISION: &str = "owned-lyrics-recording:\"1\"";
pub const DOCUMENT_REVISION: &str = "owned-lyric-document:\"1\"";
const PLAYLIST: &str = "opaque:playlist/owned-lyrics";
const SNAPSHOT: &str = "owned-lyrics-snapshot:1";

struct Observation {
    path: String,
    headers: HeaderMap,
    body: Value,
}

struct Fixture {
    requests: Mutex<Vec<Observation>>,
    descriptor: RwLock<Value>,
    lyrics: RwLock<LyricsReply>,
    held_lyrics: Mutex<Option<Arc<HeldBody>>>,
    held_descriptor: Mutex<Option<Arc<HeldBody>>>,
    chunked_lengths: Mutex<Vec<(usize, bool)>>,
}

#[derive(Clone)]
enum LyricsReply {
    Json(Value),
    Chunked(Vec<u8>),
}

pub struct Provider {
    pub url: String,
    state: Arc<Fixture>,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Provider {
    pub async fn start() -> Self {
        let state = Arc::new(Fixture {
            requests: Mutex::new(Vec::new()),
            descriptor: RwLock::new(json!({"lyrics_version": 1})),
            lyrics: RwLock::new(LyricsReply::Json(json!(response(true)))),
            held_lyrics: Mutex::new(None),
            held_descriptor: Mutex::new(None),
            chunked_lengths: Mutex::new(Vec::new()),
        });
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/owned/", listener.local_addr().unwrap());
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

    pub async fn negative(&self) {
        *self.state.lyrics.write().await = LyricsReply::Json(json!(response(false)));
    }

    pub async fn lyrics(&self, value: Value) {
        assert!(serde_json::to_vec(&value).unwrap().len() <= 524_288);
        *self.state.lyrics.write().await = LyricsReply::Json(value);
    }

    pub async fn chunked_lyrics(&self, bytes: Vec<u8>) {
        assert!(bytes.len() <= 524_288);
        *self.state.lyrics.write().await = LyricsReply::Chunked(bytes);
    }

    pub async fn assert_chunked_lyrics_lengths(&self, lengths: &[usize]) {
        let expected = lengths
            .iter()
            .map(|length| (*length, true))
            .collect::<Vec<_>>();
        assert_eq!(*self.state.chunked_lengths.lock().await, expected);
    }

    pub async fn descriptor(&self, value: Value) {
        *self.state.descriptor.write().await = value;
    }

    pub async fn hold_lyrics(&self) -> Arc<HeldBody> {
        let mut held = self.state.held_lyrics.lock().await;
        assert!(held.is_none());
        let gate = HeldBody::new();
        *held = Some(gate.clone());
        gate
    }

    pub async fn hold_descriptor(&self) -> Arc<HeldBody> {
        let mut held = self.state.held_descriptor.lock().await;
        assert!(held.is_none());
        let gate = HeldBody::new();
        *held = Some(gate.clone());
        gate
    }

    pub async fn release_descriptor(&self) {
        if let Some(held) = self.state.held_descriptor.lock().await.take() {
            held.release();
        }
    }

    pub async fn release_lyrics(&self) {
        if let Some(held) = self.state.held_lyrics.lock().await.take() {
            held.release();
        }
    }

    pub async fn assert_lookups(&self, expected: usize) {
        self.assert_calls(expected, expected).await;
    }

    pub async fn assert_calls(&self, descriptors: usize, lyrics: usize) {
        let requests = self.state.requests.lock().await;
        assert_eq!(requests.len(), 4 + descriptors + lyrics);
        for (path, count) in [
            ("/owned/v1/manifest", 1),
            ("/owned/v1/playlists/detail", 2),
            ("/owned/v1/playlists/items", 1),
            ("/owned/v1/lyrics/capabilities", descriptors),
            ("/owned/v1/lyrics", lyrics),
        ] {
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == path)
                    .count(),
                count
            );
        }
        for request in requests
            .iter()
            .filter(|request| request.path == "/owned/v1/lyrics")
        {
            assert!(
                request.body
                    == json!(LyricsRequest {
                        recording: reference()
                    })
            );
            assert!(
                request.headers.get("authorization").unwrap() == format!("Bearer {TOKEN}").as_str()
            );
            assert!(request.headers.get("x-hito-plugin-protocol").unwrap() == "1");
            assert!(request.headers.get("content-type").unwrap() == "application/json");
        }
    }

    pub async fn finish(mut self) {
        self.release_descriptor().await;
        self.release_lyrics().await;
        let _ = self.shutdown.take().unwrap().send(());
        tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.task)
            .await
            .expect("The owned lyric provider must close within its shutdown deadline")
            .expect("The owned lyric provider must join shutdown");
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn reference() -> ProviderRecordingRef {
    ProviderRecordingRef {
        id: PROVIDER_ID.into(),
        revision: Some(RECORDING_REVISION.into()),
    }
}

pub fn response(present: bool) -> LyricsResponse {
    LyricsResponse {
        recording: reference(),
        lyrics: present.then(|| TimedLyrics {
            id: "owned-lyric-document".into(),
            rendition_id: "owned-lyric-rendition".into(),
            revision: Some(DOCUMENT_REVISION.into()),
            rendition_label: None,
            language: Some("pt".into()),
            duration_ms: Some(4000),
            attribution: None,
            lines: vec![
                TimedLyricLine {
                    start_ms: 0,
                    end_ms: 1000,
                    text: LINE.into(),
                },
                TimedLyricLine {
                    start_ms: 2000,
                    end_ms: 4000,
                    text: "Owned final lyric line".into(),
                },
            ],
        }),
    }
}

async fn exchange(State(state): State<Arc<Fixture>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path();
    let expected_method = if matches!(path, "/owned/v1/manifest" | "/owned/v1/lyrics/capabilities")
    {
        Method::GET
    } else {
        Method::POST
    };
    let bytes = to_bytes(body, 4096)
        .await
        .expect("Owned provider requests must be bounded");
    assert!(parts.uri.query().is_none() && parts.method == expected_method);
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let mut requests = state.requests.lock().await;
    assert!(
        requests.len() < 32,
        "The owned provider request count must stay bounded"
    );
    requests.push(Observation {
        path: path.into(),
        headers: parts.headers,
        body,
    });
    drop(requests);
    let value = match path {
        "/owned/v1/manifest" => json!(Manifest {
            id: "org.hito.owned-timed-lyrics".into(),
            name: "Owned lyric provider".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Playlists, Capability::Lyrics].into(),
            authorization: None,
            content_policy: None,
        }),
        "/owned/v1/playlists/detail" => json!(ProviderPlaylist {
            id: PLAYLIST.into(),
            name: "Owned lyric collection".into(),
            snapshot: SNAPSHOT.into(),
            total_count: 2,
            attribution: PlaylistAttribution {
                label: "Owned protocol fixture".into()
            },
        }),
        "/owned/v1/playlists/items" => json!(PlaylistPage {
            playlist_id: PLAYLIST.into(),
            snapshot: SNAPSHOT.into(),
            next_cursor: None,
            items: (0..2)
                .map(|index| ProviderPlaylistEntry {
                    occurrence_id: Some(format!("owned-lyrics-occurrence-{index}")),
                    item: ProviderPlaylistItem::Recording {
                        recording: ProviderRecording {
                            isrc: None,
                            id: PROVIDER_ID.into(),
                            title: TITLE.into(),
                            artist: "Owned lyric performer".into(),
                            duration_ms: Some(4000),
                            release: None,
                            disc_number: None,
                            track_number: None,
                            revision: reference().revision,
                            attribution: None,
                        }
                    },
                })
                .collect(),
        }),
        "/owned/v1/lyrics/capabilities" => {
            let value = state.descriptor.read().await.clone();
            if let Some(held) = state.held_descriptor.lock().await.clone() {
                return Response::builder()
                    .header("content-type", "application/json")
                    .body(held.body(serde_json::to_vec(&value).unwrap()))
                    .unwrap();
            }
            value
        }
        "/owned/v1/lyrics" => {
            let value = match state.lyrics.read().await.clone() {
                LyricsReply::Json(value) => value,
                LyricsReply::Chunked(bytes) => {
                    let length = bytes.len();
                    let response = body::chunked_json(bytes);
                    state
                        .chunked_lengths
                        .lock()
                        .await
                        .push((length, !response.headers().contains_key("content-length")));
                    return response;
                }
            };
            if let Some(held) = state.held_lyrics.lock().await.clone() {
                return Response::builder()
                    .header("content-type", "application/json")
                    .body(held.body(serde_json::to_vec(&value).unwrap()))
                    .unwrap();
            }
            value
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Json(value).into_response()
}
