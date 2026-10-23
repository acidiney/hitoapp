use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{Method, Response, StatusCode},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle};

pub const ENCLOSURE_TARGET: &str = "/episode.wav?owned=range-fixture";
pub const FIRST: usize = 16;
pub const LAST: usize = 63;
pub const SUFFIX: usize = 29;

#[derive(Clone)]
pub struct Seen {
    pub method: Method,
    pub target: String,
    pub headers: BTreeMap<String, String>,
    pub sensitive_headers: bool,
}

struct StateData {
    feed: String,
    audio: Vec<u8>,
    seen: Mutex<Vec<Seen>>,
}

pub struct PodcastRangeServer {
    pub origin: String,
    pub feed_url: String,
    state: Arc<StateData>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl PodcastRangeServer {
    pub async fn start(audio: Vec<u8>) -> Self {
        assert!(audio.len() > LAST && audio.len() > SUFFIX);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let feed_url = format!("{origin}/feed.xml");
        let feed = format!(
            r#"<rss version="2.0"><channel>
            <title>Owned range feed</title><description>Owned deterministic WAV.</description>
            <item><guid isPermaLink="false">owned-range-episode</guid><title>Owned range episode</title>
            <enclosure url="{origin}{ENCLOSURE_TARGET}" type="audio/wav" length="{}"/>
            </item></channel></rss>"#,
            audio.len()
        );
        let state = Arc::new(StateData {
            feed,
            audio,
            seen: Mutex::new(Vec::new()),
        });
        let app = Router::new().fallback(exchange).with_state(state.clone());
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
            origin,
            feed_url,
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.state.seen.lock().unwrap().clone()
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

impl Drop for PodcastRangeServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exchange(State(state): State<Arc<StateData>>, request: Request) -> Response<Body> {
    let headers = request.headers();
    let captured = [
        "host",
        "accept",
        "accept-encoding",
        "connection",
        "user-agent",
        "range",
    ]
    .into_iter()
    .filter_map(|name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|value| (name.to_owned(), value.to_owned()))
    })
    .collect();
    state.seen.lock().unwrap().push(Seen {
        method: request.method().clone(),
        target: request.uri().to_string(),
        headers: captured,
        sensitive_headers: headers.keys().any(|name| {
            matches!(
                name.as_str(),
                "authorization" | "cookie" | "cookie2" | "proxy-authorization"
            ) || name.as_str().starts_with("x-hito-")
        }),
    });
    if request.method() == Method::GET && request.uri() == "/feed.xml" {
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/rss+xml")
            .header("content-length", state.feed.len())
            .body(Body::from(state.feed.clone()))
            .unwrap();
    }
    if request.uri() != ENCLOSURE_TARGET {
        return empty(StatusCode::NOT_FOUND);
    }
    let total = state.audio.len();
    let range = headers.get("range").and_then(|value| value.to_str().ok());
    let (status, body, selection) = if request.method() == Method::HEAD && range.is_none() {
        (StatusCode::OK, Vec::new(), None)
    } else if request.method() == Method::GET {
        if range == Some(format!("bytes={FIRST}-{LAST}").as_str()) {
            (
                StatusCode::PARTIAL_CONTENT,
                state.audio[FIRST..=LAST].to_vec(),
                Some((FIRST, LAST)),
            )
        } else if range == Some(format!("bytes=-{SUFFIX}").as_str()) {
            (
                StatusCode::PARTIAL_CONTENT,
                state.audio[total - SUFFIX..].to_vec(),
                Some((total - SUFFIX, total - 1)),
            )
        } else if range == Some(format!("bytes={}-", total + 5).as_str()) {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header("content-range", format!("bytes */{total}"))
                .header("content-length", 0)
                .body(Body::empty())
                .unwrap();
        } else {
            return empty(StatusCode::BAD_REQUEST);
        }
    } else {
        return empty(StatusCode::BAD_REQUEST);
    };
    let length = if request.method() == Method::HEAD {
        total
    } else {
        body.len()
    };
    let mut response = Response::builder()
        .status(status)
        .header("content-type", "audio/wav")
        .header("content-length", length)
        .header("accept-ranges", "bytes");
    if let Some((first, last)) = selection {
        response = response.header("content-range", format!("bytes {first}-{last}/{total}"));
    }
    response.body(Body::from(body)).unwrap()
}

fn empty(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-length", 0)
        .body(Body::empty())
        .unwrap()
}
