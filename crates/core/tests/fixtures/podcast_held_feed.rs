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
    sync::{Mutex, oneshot, watch},
    task::JoinHandle,
};

struct Feed {
    xml: String,
    requests: AtomicUsize,
    unexpected: AtomicUsize,
    held_finished: AtomicUsize,
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: watch::Sender<bool>,
}

pub struct HeldFeedServer {
    pub origin: String,
    pub feed_url: String,
    state: Arc<Feed>,
    entered: Option<oneshot::Receiver<()>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl HeldFeedServer {
    pub async fn start(enclosure_url: &str, length: usize) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (entered, observed) = oneshot::channel();
        let (release, _) = watch::channel(false);
        let state = Arc::new(Feed {
            xml: format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Owned held feed</title>
<description>Deterministic worker retirement.</description><managingEditor>Owned held-feed author</managingEditor>
<item><guid isPermaLink="false">owned-held-feed-episode</guid><title>Owned held-feed episode</title>
<enclosure url="{enclosure_url}" length="{length}" type="audio/wav" />
</item></channel></rss>"#
            ),
            requests: AtomicUsize::new(0),
            unexpected: AtomicUsize::new(0),
            held_finished: AtomicUsize::new(0),
            entered: Mutex::new(Some(entered)),
            release,
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
            feed_url: format!("{origin}/feed.xml"),
            origin,
            state,
            entered: Some(observed),
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub async fn wait_for_held_request(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.take().unwrap())
            .await
            .expect("the actual second feed request must reach the owned response gate")
            .expect("the owned handler must signal its pending request");
    }

    pub fn requests(&self) -> usize {
        self.state.requests.load(Ordering::SeqCst)
    }

    pub fn held_finished(&self) -> usize {
        self.state.held_finished.load(Ordering::SeqCst)
    }

    pub fn unexpected(&self) -> usize {
        self.state.unexpected.load(Ordering::SeqCst)
    }

    pub async fn finish(&mut self) -> bool {
        self.state.release.send_replace(true);
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

impl Drop for HeldFeedServer {
    fn drop(&mut self) {
        self.state.release.send_replace(true);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exchange(State(state): State<Arc<Feed>>, request: Request) -> Response<Body> {
    let sequence = state.requests.fetch_add(1, Ordering::SeqCst) + 1;
    let headers = request.headers();
    let valid = request.method() == Method::GET
        && request.uri().path() == "/feed.xml"
        && request.uri().query().is_none()
        && headers
            .get("accept-encoding")
            .is_some_and(|value| value == "identity")
        && headers
            .get("connection")
            .is_some_and(|value| value == "close")
        && headers
            .get("accept")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("application/rss+xml"))
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
    if !valid {
        state.unexpected.fetch_add(1, Ordering::SeqCst);
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap();
    }
    if sequence == 2 {
        let mut released = state.release.subscribe();
        if let Some(entered) = state.entered.lock().await.take() {
            let _ = entered.send(());
        }
        loop {
            let release_observed = *released.borrow_and_update();
            if release_observed {
                break;
            }
            if released.changed().await.is_err() {
                break;
            }
        }
        state.held_finished.fetch_add(1, Ordering::SeqCst);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/rss+xml")
        .header("content-length", state.xml.len())
        .body(Body::from(state.xml.clone()))
        .unwrap()
}
