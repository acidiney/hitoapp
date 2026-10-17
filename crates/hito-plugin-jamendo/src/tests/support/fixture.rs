use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tracing::instrument::WithSubscriber;
use url::Url;

pub const CLIENT_ID: &str = "private-owned-jamendo-client";
pub const QUERY: &str = "Owned música & + # ?";
pub const TITLE: &str = "Owned signal";
pub const ARTIST: &str = "Owned performer";
pub const ALBUM: &str = "Owned album";

pub struct Server {
    pub base: Url,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Server {
    pub async fn start(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener setup failed");
        Self::serve(listener, router)
    }

    pub async fn start_with_dispatch(router: Router, dispatch: tracing::Dispatch) -> Self {
        let router = router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let dispatch = dispatch.clone();
                async move { next.run(request).with_subscriber(dispatch).await }
            },
        ));
        Self::start(router).await
    }

    fn serve(listener: tokio::net::TcpListener, router: Router) -> Self {
        let address = listener.local_addr().expect("fixture address failed");
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        Self {
            base: Url::parse(&format!("http://{address}/")).unwrap(),
            task,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct Provider {
    pub base: Url,
    pub wav: Vec<u8>,
    state: Arc<ProviderState>,
    _server: Server,
    _directory: tempfile::TempDir,
}

struct ProviderState {
    base: Url,
    wav: Vec<u8>,
    audio_override: Mutex<Option<Url>>,
    requests: AtomicUsize,
    api: AtomicUsize,
    media: AtomicUsize,
    downloads: AtomicUsize,
    artwork: AtomicUsize,
    rejected: AtomicBool,
}

impl Provider {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().expect("owned audio directory failed");
        let wav = owned_wav();
        std::fs::write(directory.path().join("owned.wav"), &wav)
            .expect("owned audio fixture write failed");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("provider fixture listener failed");
        let address = listener
            .local_addr()
            .expect("provider fixture address failed");
        let base = Url::parse(&format!("http://{address}/")).unwrap();
        let state = Arc::new(ProviderState {
            base: base.clone(),
            wav: wav.clone(),
            audio_override: Mutex::new(None),
            requests: AtomicUsize::new(0),
            api: AtomicUsize::new(0),
            media: AtomicUsize::new(0),
            downloads: AtomicUsize::new(0),
            artwork: AtomicUsize::new(0),
            rejected: AtomicBool::new(false),
        });
        let server = Server::serve(
            listener,
            Router::new()
                .route("/v3.0/tracks/", get(tracks))
                .route("/audio/owned.wav", get(audio))
                .route("/download/owned.wav", get(download))
                .route("/artwork/owned.png", get(artwork))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    count_request,
                ))
                .with_state(state.clone()),
        );
        Self {
            base,
            wav,
            state,
            _server: server,
            _directory: directory,
        }
    }

    pub fn api_requests(&self) -> usize {
        self.state.api.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }

    pub fn replace_audio_url(&self, endpoint: Url) {
        *self
            .state
            .audio_override
            .lock()
            .expect("fixture audio control failed") = Some(endpoint);
    }

    pub fn media_requests(&self) -> usize {
        self.state.media.load(Ordering::SeqCst)
    }

    pub fn download_requests(&self) -> usize {
        self.state.downloads.load(Ordering::SeqCst)
    }

    pub fn artwork_requests(&self) -> usize {
        self.state.artwork.load(Ordering::SeqCst)
    }

    pub fn rejected_request(&self) -> bool {
        self.state.rejected.load(Ordering::SeqCst)
    }
}

async fn tracks(
    State(state): State<Arc<ProviderState>>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    state.api.fetch_add(1, Ordering::SeqCst);
    let permitted = [
        "client_id",
        "format",
        "search",
        "id",
        "limit",
        "offset",
        "type",
        "audioformat",
    ];
    let selection = query.get("id").is_some_and(|id| id == "42") && !query.contains_key("search");
    let search = query.get("search").is_some_and(|search| search == QUERY)
        && !query.contains_key("id")
        && query.get("limit").is_some_and(|limit| limit == "10")
        && query.get("offset").is_some_and(|offset| offset == "0");
    if !private_headers_absent(&headers)
        || query.get("client_id").is_none_or(|id| id != CLIENT_ID)
        || query.get("format").is_none_or(|format| format != "json")
        || query
            .get("type")
            .is_none_or(|kind| kind != "single albumtrack")
        || query
            .get("audioformat")
            .is_none_or(|format| format != "mp32")
        || query.keys().any(|key| !permitted.contains(&key.as_str()))
        || !(selection || search)
    {
        state.rejected.store(true, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    let audio = state
        .audio_override
        .lock()
        .expect("fixture audio control failed")
        .clone()
        .unwrap_or_else(|| state.base.join("audio/owned.wav?private=stream").unwrap());
    Json(json!({
        "headers": {"status": "success", "code": 0, "error_message": "", "warnings": "", "results_count": 1},
        "results": [{
            "id": "42", "name": TITLE, "artist_id": "17", "artist_name": ARTIST,
            "album_id": "23", "album_name": ALBUM, "duration": 1, "position": 3,
            "releasedate": "2025-01-01", "license_ccurl": "https://creativecommons.org/licenses/by/4.0/",
            "audio": audio,
            "audiodownload": state.base.join("download/owned.wav?private=download").unwrap(),
            "audiodownload_allowed": true,
            "image": state.base.join("artwork/owned.png?private=artwork").unwrap(),
            "album_image": state.base.join("artwork/owned.png?private=artwork").unwrap(),
            "shareurl": "https://www.jamendo.com/track/42", "shorturl": "https://jamen.do/t/42"
        }]
    }))
    .into_response()
}

async fn count_request(
    State(state): State<Arc<ProviderState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    next.run(request).await
}

async fn audio(State(state): State<Arc<ProviderState>>, headers: HeaderMap) -> Response {
    state.media.fetch_add(1, Ordering::SeqCst);
    if !private_headers_absent(&headers) {
        state.rejected.store(true, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    (
        [
            ("content-type", "audio/wav"),
            ("accept-ranges", "bytes"),
            ("set-cookie", "private-provider-cookie"),
        ],
        state.wav.clone(),
    )
        .into_response()
}

async fn download(State(state): State<Arc<ProviderState>>) -> StatusCode {
    state.downloads.fetch_add(1, Ordering::SeqCst);
    StatusCode::FORBIDDEN
}

async fn artwork(State(state): State<Arc<ProviderState>>) -> StatusCode {
    state.artwork.fetch_add(1, Ordering::SeqCst);
    StatusCode::NOT_FOUND
}

fn private_headers_absent(headers: &HeaderMap) -> bool {
    !headers.contains_key("authorization")
        && !headers.contains_key("cookie")
        && headers
            .get("accept-encoding")
            .is_some_and(|value| value == "identity")
}

pub fn assert_private_urls_absent(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).expect("response was not JSON UTF-8");
    for forbidden in [CLIENT_ID, "private-", "http://", "https://", "127.0.0.1"] {
        assert!(
            !text.contains(forbidden),
            "provider response exposed private transport data"
        );
    }
}

fn owned_wav() -> Vec<u8> {
    let samples: Vec<u8> = (0..8_000u32)
        .flat_map(|index| {
            let phase = index as f64 * 440.0 * std::f64::consts::TAU / 8_000.0;
            ((phase.sin() * 4_000.0) as i16).to_le_bytes()
        })
        .collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + samples.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8_000u32.to_le_bytes());
    wav.extend(16_000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((samples.len() as u32).to_le_bytes());
    wav.extend(samples);
    wav
}
