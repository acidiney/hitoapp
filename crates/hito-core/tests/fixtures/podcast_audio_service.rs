use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{Method, Response, StatusCode},
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
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let feed_url = format!("{origin}/feed.xml");
        let enclosure_url = format!("{origin}/episode.wav?{ENCLOSURE_QUERY}");
        let state = Arc::new(Feed {
            snapshot: RwLock::new(Snapshot {
                xml: String::new(),
                audio: Vec::new(),
            }),
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
    let valid = request.method() == Method::GET
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
            "range",
        ]
        .iter()
        .all(|name| !headers.contains_key(*name));
    let snapshot = state.snapshot.read().await;
    let (mime, body) = match (request.uri().path(), request.uri().query()) {
        ("/feed.xml", None)
            if valid
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
