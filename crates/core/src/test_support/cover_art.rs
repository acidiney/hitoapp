use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone)]
pub enum Reply {
    Png(Vec<u8>),
    NotFound,
    Unavailable,
}

#[derive(Default)]
struct State_ {
    replies: HashMap<String, Reply>,
    requests: Vec<String>,
}

/// An owned loopback stand-in for the Cover Art Archive.
pub struct FakeCoverArt {
    pub endpoint: url::Url,
    state: Arc<Mutex<State_>>,
    server: JoinHandle<()>,
}

impl FakeCoverArt {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(State_::default()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let router = Router::new().fallback(respond).with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            endpoint,
            state,
            server,
        }
    }

    /// Answers the front cover of `kind` (`release` or `release-group`) and identifier.
    pub fn front(&self, kind: &str, mbid: &str, reply: Reply) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("{kind}/{mbid}"), reply);
    }

    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().requests.clone()
    }
}

impl Drop for FakeCoverArt {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn respond(State(state): State<Arc<Mutex<State_>>>, request: Request) -> Response {
    let path = request.uri().path().trim_start_matches('/').to_owned();
    let reply = {
        let mut state = state.lock().unwrap();
        state.requests.push(request.uri().path().to_owned());
        let key = path.trim_end_matches("/front-500");
        state.replies.get(key).cloned()
    };
    let builder = Response::builder();
    match reply {
        Some(Reply::Png(bytes)) => builder
            .header("content-type", "image/png")
            .body(bytes.into())
            .unwrap(),
        Some(Reply::Unavailable) => builder
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body("".into())
            .unwrap(),
        Some(Reply::NotFound) | None => builder
            .status(StatusCode::NOT_FOUND)
            .body("".into())
            .unwrap(),
    }
}
