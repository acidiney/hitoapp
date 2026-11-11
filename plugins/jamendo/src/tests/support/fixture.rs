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
use url::Url;

pub const CLIENT_ID: &str = "private-owned-jamendo-client";
pub const QUERY: &str = "Owned música & + # ?";
pub const TITLE: &str = "Owned signal";
pub const ARTIST: &str = "Owned performer";
pub const ALBUM: &str = "Owned album";
pub const CONTENT_URL: &str = "https://www.jamendo.com/track/42";

pub use super::server::Server;

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
    ranges: AtomicUsize,
    downloads: AtomicUsize,
    artwork: AtomicUsize,
    albums: AtomicUsize,
    album_changed: AtomicBool,
    rejected: AtomicBool,
}

impl Provider {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().expect("owned audio directory failed");
        let wav = owned_wav();
        std::fs::write(directory.path().join("owned.wav"), &wav)
            .expect("owned audio fixture write failed");
        let state_wav = wav.clone();
        let (server, state) = Server::build(move |base| {
            let state = Arc::new(ProviderState {
                base: base.clone(),
                wav: state_wav,
                audio_override: Mutex::new(None),
                requests: AtomicUsize::new(0),
                api: AtomicUsize::new(0),
                media: AtomicUsize::new(0),
                ranges: AtomicUsize::new(0),
                downloads: AtomicUsize::new(0),
                artwork: AtomicUsize::new(0),
                albums: AtomicUsize::new(0),
                album_changed: AtomicBool::new(false),
                rejected: AtomicBool::new(false),
            });
            let router = Router::new()
                .route("/v3.0/tracks/", get(tracks))
                .route("/v3.0/albums/", get(albums))
                .route("/v3.0/albums/tracks/", get(album_tracks))
                .route("/audio/owned.wav", get(audio))
                .route("/download/owned.wav", get(download))
                .route("/artwork/owned.png", get(artwork))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    count_request,
                ))
                .with_state(state.clone());
            (router, state)
        })
        .await;
        let base = server.base.clone();
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

    pub fn range_requests(&self) -> usize {
        self.state.ranges.load(Ordering::SeqCst)
    }

    pub fn download_requests(&self) -> usize {
        self.state.downloads.load(Ordering::SeqCst)
    }

    pub fn artwork_requests(&self) -> usize {
        self.state.artwork.load(Ordering::SeqCst)
    }

    pub fn album_requests(&self) -> usize {
        self.state.albums.load(Ordering::SeqCst)
    }

    pub fn change_album(&self) {
        self.state.album_changed.store(true, Ordering::SeqCst);
    }

    pub fn rejected_request(&self) -> bool {
        self.state.rejected.load(Ordering::SeqCst)
    }

    pub async fn shutdown_report(self) -> super::server::Shutdown {
        self._server.shutdown_report().await
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
            "shareurl": format!("https://share.invalid/track/99?private=share&client_id={CLIENT_ID}"),
            "shorturl": format!("https://short.invalid/t/99?private=short&client_id={CLIENT_ID}")
        }]
    }))
    .into_response()
}

async fn albums(
    State(state): State<Arc<ProviderState>>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    state.albums.fetch_add(1, Ordering::SeqCst);
    let permitted = ["client_id", "format", "order", "limit", "imagesize"];
    if !private_headers_absent(&headers)
        || query.get("client_id").is_none_or(|id| id != CLIENT_ID)
        || query.get("format").is_none_or(|format| format != "json")
        || query
            .get("order")
            .is_none_or(|order| order != "popularity_week")
        || query.get("limit").is_none_or(|limit| limit != "12")
        || query.keys().any(|key| !permitted.contains(&key.as_str()))
    {
        state.rejected.store(true, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(json!({
        "headers": {"status": "success", "code": 0, "error_message": "", "warnings": "", "results_count": 2},
        "results": [
            {"id": "23", "name": ALBUM, "artist_id": "17", "artist_name": ARTIST,
             "releasedate": "2025-01-01",
             "image": state.base.join("artwork/owned.png?private=artwork").unwrap(),
             "shareurl": format!("https://share.invalid/album/23?private=share&client_id={CLIENT_ID}")},
            {"id": "24", "name": "Second owned album", "artist_id": "18", "artist_name": "Second performer",
             "releasedate": "2025-02-01",
             "image": state.base.join("artwork/owned.png?private=artwork").unwrap()}
        ]
    }))
    .into_response()
}

async fn album_tracks(
    State(state): State<Arc<ProviderState>>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    state.albums.fetch_add(1, Ordering::SeqCst);
    let permitted = ["client_id", "format", "id", "audioformat"];
    if !private_headers_absent(&headers)
        || query.get("client_id").is_none_or(|id| id != CLIENT_ID)
        || query.get("format").is_none_or(|format| format != "json")
        || query
            .get("audioformat")
            .is_none_or(|format| format != "mp32")
        || query.keys().any(|key| !permitted.contains(&key.as_str()))
    {
        state.rejected.store(true, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    let id = query.get("id").map(String::as_str).unwrap_or_default();
    if id != "23" {
        return Json(json!({
            "headers": {"status": "success", "code": 0, "error_message": "", "warnings": "", "results_count": 0},
            "results": []
        }))
        .into_response();
    }
    let audio = state.base.join("audio/owned.wav?private=stream").unwrap();
    let mut tracks = vec![
        json!({"id": "42", "name": TITLE, "duration": 1, "position": 1, "audio": audio,
               "license_ccurl": "https://creativecommons.org/licenses/by/4.0/"}),
        json!({"id": "43", "name": "Owned second signal", "duration": 2, "position": 2, "audio": audio}),
    ];
    if state.album_changed.load(Ordering::SeqCst) {
        tracks.push(json!({"id": "44", "name": "Owned late signal", "duration": 3, "position": 3, "audio": audio}));
    }
    Json(json!({
        "headers": {"status": "success", "code": 0, "error_message": "", "warnings": "", "results_count": 1},
        "results": [{
            "id": "23", "name": ALBUM, "artist_id": "17", "artist_name": ARTIST,
            "releasedate": "2025-01-01",
            "image": state.base.join("artwork/owned.png?private=artwork").unwrap(),
            "tracks": tracks
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
    let mut response = Response::builder()
        .header("content-type", "audio/wav")
        .header("accept-ranges", "bytes")
        .header("set-cookie", "private-provider-cookie");
    let body = if headers.contains_key("range") {
        state.ranges.fetch_add(1, Ordering::SeqCst);
        let Some((first, last)) = audio_range(&headers, state.wav.len()) else {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header("content-range", format!("bytes */{}", state.wav.len()))
                .body(axum::body::Body::empty())
                .unwrap();
        };
        response = response.status(StatusCode::PARTIAL_CONTENT).header(
            "content-range",
            format!("bytes {first}-{last}/{}", state.wav.len()),
        );
        state.wav[first..=last].to_vec()
    } else {
        state.wav.clone()
    };
    response
        .header("content-length", body.len())
        .body(body.into())
        .unwrap()
}

fn audio_range(headers: &HeaderMap, length: usize) -> Option<(usize, usize)> {
    let mut values = headers.get_all("range").iter();
    let raw = values.next()?.to_str().ok()?;
    if values.next().is_some() || raw.len() > 128 || length == 0 {
        return None;
    }
    let (first, last) = raw.strip_prefix("bytes=")?.split_once('-')?;
    let decimal = |value: &str| -> Option<usize> {
        (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| value.parse().ok())
            .flatten()
    };
    if first.is_empty() {
        let suffix = decimal(last).filter(|suffix| *suffix > 0)?;
        return Some((length.saturating_sub(suffix), length - 1));
    }
    let first = decimal(first)?;
    let last = if last.is_empty() {
        length - 1
    } else {
        decimal(last)?.min(length - 1)
    };
    (first <= last && first < length).then_some((first, last))
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
    let mut response: serde_json::Value =
        serde_json::from_slice(bytes).expect("response was not JSON UTF-8");
    for path in [
        "/attribution/content_url",
        "/items/0/attribution/content_url",
    ] {
        if let Some(content_url) = response.pointer_mut(path) {
            assert_eq!(
                content_url.as_str(),
                Some(CONTENT_URL),
                "provider backlink did not use the canonical recording page"
            );
            *content_url = serde_json::Value::Null;
        }
    }
    let text = serde_json::to_string(&response).expect("response JSON serialization failed");
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
