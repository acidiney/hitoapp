use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, Method, Response, StatusCode},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

pub const ETAG: &str = "W/\"owned-original-version\"";
pub const LAST_MODIFIED: &str = "Fri, 16 Oct 2026 10:00:00 GMT";
pub const AUTHOR: &str = "Owned conditional podcast author";
pub const FIRST_TITLE: &str = "Owned newer episode";
pub const FEED_TARGET: &str = "/feed.xml?owned=redirect-original";
pub const MOVED_TARGET: &str = "/different.xml?owned=redirect-other-resource";
pub const OTHER_ETAG: &str = "\"owned-other-resource-version\"";
pub const OTHER_MODIFIED: &str = "Sat, 17 Oct 2026 10:00:00 GMT";
pub const AUDIO_TARGET: &str = "/episode.wav?owned=redirect-audio";

#[derive(Clone, Debug)]
pub struct ObservedRequest {
    pub method: Method,
    pub target: String,
    pub headers: HeaderMap,
}

struct Feed {
    xml: String,
    audio: Vec<u8>,
    redirect: AtomicBool,
    requests: Mutex<Vec<ObservedRequest>>,
    enclosures: AtomicUsize,
    unexpected: AtomicUsize,
}

pub struct PodcastConditionalRedirectServer {
    pub origin: String,
    pub feed_url: String,
    state: Arc<Feed>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl PodcastConditionalRedirectServer {
    pub async fn start(audio: Vec<u8>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Feed {
            xml: feed_xml(&format!("{origin}{AUDIO_TARGET}"), audio.len()),
            audio,
            redirect: AtomicBool::new(false),
            requests: Mutex::new(Vec::new()),
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
        Self {
            feed_url: format!("{origin}{FEED_TARGET}"),
            origin,
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn redirect_to_unrequested_304(&self) {
        self.state.redirect.store(true, Ordering::SeqCst);
    }

    pub async fn requests(&self) -> Vec<ObservedRequest> {
        self.state.requests.lock().await.clone()
    }

    pub fn enclosure_requests(&self) -> usize {
        self.state.enclosures.load(Ordering::SeqCst)
    }

    pub fn unexpected_requests(&self) -> usize {
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

impl Drop for PodcastConditionalRedirectServer {
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
    let target = request.uri().to_string();
    {
        let mut requests = state.requests.lock().await;
        if requests.len() >= 8 {
            return invalid(&state);
        }
        requests.push(ObservedRequest {
            method: request.method().clone(),
            target: target.clone(),
            headers: request.headers().clone(),
        });
    }
    if request.method() != Method::GET {
        return invalid(&state);
    }
    match target.as_str() {
        FEED_TARGET if state.redirect.load(Ordering::SeqCst) => Response::builder()
            .status(StatusCode::MOVED_PERMANENTLY)
            .header("location", MOVED_TARGET)
            .body(Body::empty())
            .unwrap(),
        FEED_TARGET => Response::builder()
            .status(StatusCode::OK)
            .header("etag", ETAG)
            .header("last-modified", LAST_MODIFIED)
            .header("content-type", "application/rss+xml")
            .header("content-length", state.xml.len())
            .body(Body::from(state.xml.clone()))
            .unwrap(),
        MOVED_TARGET if state.redirect.load(Ordering::SeqCst) => Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("etag", OTHER_ETAG)
            .header("last-modified", OTHER_MODIFIED)
            .body(Body::empty())
            .unwrap(),
        AUDIO_TARGET => {
            state.enclosures.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "audio/wav")
                .header("content-length", state.audio.len())
                .header("accept-ranges", "bytes")
                .body(Body::from(state.audio.clone()))
                .unwrap()
        }
        _ => invalid(&state),
    }
}

fn invalid(state: &Feed) -> Response<Body> {
    state.unexpected.fetch_add(1, Ordering::SeqCst);
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .unwrap()
}

fn feed_xml(enclosure: &str, length: usize) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Owned conditional feed</title>
<description>Owned conditional refresh fixture.</description><managingEditor>{AUTHOR}</managingEditor>
<item><guid isPermaLink="false">owned-newer-guid</guid><title>{FIRST_TITLE}</title>
<pubDate>Fri, 16 Oct 2026 12:00:00 +0000</pubDate>
<enclosure url="{enclosure}" length="{length}" type="audio/wav" /></item>
<item><guid isPermaLink="false">owned-older-guid</guid><title>Owned older episode</title>
<pubDate>Thu, 15 Oct 2026 12:00:00 +0000</pubDate>
<enclosure url="{enclosure}" length="{length}" type="audio/wav" /></item>
</channel></rss>"#
    )
}
