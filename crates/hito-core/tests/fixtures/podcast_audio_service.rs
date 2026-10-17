use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, Method, Response, StatusCode},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{RwLock, oneshot},
    task::JoinHandle,
};

pub const TITLE: &str = "Owned playable episode";
pub const AUTHOR: &str = "Owned podcast performer";
const ENCLOSURE_QUERY: &str = "owned=podcast-private-enclosure-canary";
const REPLACEMENT_QUERY: &str = "owned=podcast-private-enclosure-canary-replacement";

struct Snapshot {
    xml: String,
    audio: Vec<Vec<u8>>,
}

struct Feed {
    snapshot: RwLock<Snapshot>,
    ranges: bool,
    requests: AtomicUsize,
    feeds: AtomicUsize,
    enclosures: AtomicUsize,
    unexpected: AtomicUsize,
}

pub struct PodcastAudioServer {
    pub origin: String,
    pub feed_url: String,
    pub enclosure_url: String,
    state: Arc<Feed>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl PodcastAudioServer {
    pub async fn start(audio: Vec<u8>) -> Self {
        Self::start_owned(audio, false).await
    }

    #[allow(dead_code)]
    pub async fn start_with_ranges(audio: Vec<u8>) -> Self {
        Self::start_owned(audio, true).await
    }

    async fn start_owned(audio: Vec<u8>, ranges: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let feed_url = format!("{origin}/feed.xml");
        let enclosure_url = format!("{origin}/episode.wav?{ENCLOSURE_QUERY}");
        let state = Arc::new(Feed {
            snapshot: RwLock::new(Snapshot {
                xml: String::new(),
                audio: Vec::new(),
            }),
            ranges,
            requests: AtomicUsize::new(0),
            feeds: AtomicUsize::new(0),
            enclosures: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
        });
        let router = Router::new().fallback(exchange).with_state(state.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .is_ok()
        });
        let server = Self {
            origin,
            feed_url,
            enclosure_url,
            state,
            stop: Some(stop),
            task: Some(task),
        };
        server.publish_enclosure(audio).await;
        server
    }

    pub async fn publish_enclosure(&self, audio: Vec<u8>) {
        let mut snapshot = self.state.snapshot.write().await;
        assert!(
            snapshot.audio.len() < 2,
            "the owned enclosure fixture allows exactly one replacement"
        );
        let query = if snapshot.audio.is_empty() {
            ENCLOSURE_QUERY
        } else {
            REPLACEMENT_QUERY
        };
        let enclosure_url = format!("{}/episode.wav?{query}", self.origin);
        snapshot.xml = feed_xml(&enclosure_url, audio.len());
        snapshot.audio.push(audio);
    }

    pub fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }
    pub fn feed_requests(&self) -> usize {
        self.state.feeds.load(Ordering::SeqCst)
    }
    pub fn enclosure_requests(&self) -> usize {
        self.state.enclosures.load(Ordering::SeqCst)
    }
    pub fn unexpected(&self) -> usize {
        self.state.unexpected.load(Ordering::SeqCst)
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

impl Drop for PodcastAudioServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exchange(State(state): State<Arc<Feed>>, request: Request) -> Response<Body> {
    state.requests.fetch_add(1, Ordering::SeqCst);
    let headers = request.headers();
    let valid = (request.method() == Method::GET
        || (state.ranges && request.method() == Method::HEAD))
        && headers
            .get("accept-encoding")
            .is_some_and(|value| value == "identity")
        && headers
            .get("connection")
            .is_some_and(|value| value == "close")
        && headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("Hito/"))
        && [
            "authorization",
            "cookie",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ]
        .iter()
        .all(|name| !headers.contains_key(*name))
        && (state.ranges || !headers.contains_key("range"));
    let snapshot = state.snapshot.read().await;
    let (mime, body) = match (request.uri().path(), request.uri().query()) {
        ("/feed.xml", None)
            if valid
                && request.method() == Method::GET
                && !headers.contains_key("range")
                && headers
                    .get("accept")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.contains("application/rss+xml")) =>
        {
            state.feeds.fetch_add(1, Ordering::SeqCst);
            ("application/rss+xml", snapshot.xml.as_bytes().to_vec())
        }
        ("/episode.wav", Some(ENCLOSURE_QUERY) | Some(REPLACEMENT_QUERY))
            if valid
                && headers
                    .get("accept")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.contains("audio/")) =>
        {
            let index = usize::from(request.uri().query() == Some(REPLACEMENT_QUERY));
            let Some(audio) = snapshot.audio.get(index) else {
                return invalid(&state);
            };
            state.enclosures.fetch_add(1, Ordering::SeqCst);
            if state.ranges {
                return ranged_audio(&state, &request, audio);
            }
            ("audio/wav", audio.clone())
        }
        _ => return invalid(&state),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", mime)
        .header("content-length", body.len())
        .header("accept-ranges", "bytes")
        .body(Body::from(body))
        .unwrap()
}

fn ranged_audio(state: &Feed, request: &Request, audio: &[u8]) -> Response<Body> {
    let head = request.method() == Method::HEAD;
    let selection = if head {
        None
    } else {
        match audio_range(request.headers(), audio.len()) {
            Ok(selection) => selection,
            Err(StatusCode::RANGE_NOT_SATISFIABLE) => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("content-range", format!("bytes */{}", audio.len()))
                    .header("content-length", 0)
                    .header("accept-ranges", "bytes")
                    .body(Body::empty())
                    .unwrap();
            }
            Err(_) => return invalid(state),
        }
    };
    let bytes = selection
        .map(|(first, last)| &audio[first..=last])
        .unwrap_or(audio);
    let mut response = Response::builder()
        .status(if selection.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header("content-type", "audio/wav")
        .header("content-length", bytes.len())
        .header("accept-ranges", "bytes");
    if let Some((first, last)) = selection {
        response = response.header(
            "content-range",
            format!("bytes {first}-{last}/{}", audio.len()),
        );
    }
    response
        .body(if head {
            Body::empty()
        } else {
            Body::from(bytes.to_vec())
        })
        .unwrap()
}

fn audio_range(headers: &HeaderMap, length: usize) -> Result<Option<(usize, usize)>, StatusCode> {
    let mut values = headers.get_all("range").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let value = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
    if value.len() > 1024 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let value = value
        .strip_prefix("bytes=")
        .ok_or(StatusCode::BAD_REQUEST)?;
    let (first, last) = value.split_once('-').ok_or(StatusCode::BAD_REQUEST)?;
    let total = length as u64;
    if first.is_empty() {
        let suffix = decimal(last)?;
        if suffix == 0 || total == 0 {
            return Err(StatusCode::RANGE_NOT_SATISFIABLE);
        }
        return Ok(Some((total.saturating_sub(suffix) as usize, length - 1)));
    }
    let first = decimal(first)?;
    let last = if last.is_empty() {
        total.saturating_sub(1)
    } else {
        let last = decimal(last)?;
        if last < first {
            return Err(StatusCode::BAD_REQUEST);
        }
        last
    };
    if first >= total {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    Ok(Some((first as usize, last.min(total - 1) as usize)))
}

fn decimal(value: &str) -> Result<u64, StatusCode> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    value.parse().map_err(|_| StatusCode::BAD_REQUEST)
}

fn invalid(state: &Feed) -> Response<Body> {
    state.unexpected.fetch_add(1, Ordering::SeqCst);
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .unwrap()
}

fn feed_xml(enclosure_url: &str, length: usize) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Owned audio feed</title>
<description>Deterministic owned audio.</description><managingEditor>{AUTHOR}</managingEditor>
<item><guid isPermaLink="false">owned-playable-episode</guid><title>{TITLE}</title>
<enclosure url="{enclosure_url}" length="{length}" type="audio/wav" />
</item></channel></rss>"#
    )
}
