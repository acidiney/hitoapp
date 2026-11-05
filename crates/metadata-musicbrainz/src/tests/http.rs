use crate::{Error, MusicBrainzClient};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{StatusCode, Uri},
    response::Response,
};
use hito_plugin_sdk::{ArtistProfileRequest, CatalogArtistProfile};
use std::{
    collections::VecDeque,
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tracing::instrument::WithSubscriber;

pub(super) struct Reply {
    pub status: StatusCode,
    pub mime: &'static str,
    pub body: Vec<u8>,
    pub streamed: bool,
    pub location: Option<&'static str>,
}

impl Reply {
    pub fn json(body: Vec<u8>) -> Self {
        Self {
            status: StatusCode::OK,
            mime: "application/json; charset=utf-8",
            body,
            streamed: false,
            location: None,
        }
    }
}

struct StateData {
    replies: VecDeque<Reply>,
    requests: Vec<Uri>,
}

pub(super) struct Fixture {
    pub endpoint: reqwest::Url,
    state: Arc<Mutex<StateData>>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    pub async fn start(replies: Vec<Reply>) -> Self {
        let state = Arc::new(Mutex::new(StateData {
            replies: replies.into(),
            requests: Vec::new(),
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned profile protocol fixture must bind");
        let endpoint = format!(
            "http://{}/ws/2/",
            listener
                .local_addr()
                .expect("the fixture must have an address")
        )
        .parse()
        .expect("the owned profile protocol endpoint must be valid");
        let router = Router::new().fallback(respond).with_state(state.clone());
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .expect("the owned profile protocol fixture must run");
        });
        Self {
            endpoint,
            state,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub fn request_count(&self) -> usize {
        self.state
            .lock()
            .expect("the fixture state must lock")
            .requests
            .len()
    }

    pub fn requests(&self) -> Vec<Uri> {
        self.state
            .lock()
            .expect("the fixture state must lock")
            .requests
            .clone()
    }

    pub async fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(2), &mut server).await {
                Ok(result) => result.expect("the owned profile protocol fixture must stop"),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    panic!("the owned profile protocol fixture did not stop");
                }
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn respond(State(state): State<Arc<Mutex<StateData>>>, request: Request) -> Response {
    let mut state = state.lock().expect("the fixture state must lock");
    state.requests.push(request.uri().clone());
    let Some(reply) = state.replies.pop_front() else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .expect("the exhausted fixture response must be valid");
    };
    let mut response = Response::builder()
        .status(reply.status)
        .header("content-type", reply.mime);
    if let Some(location) = reply.location {
        response = response.header("location", location);
    }
    let body = if reply.streamed {
        let chunks = reply
            .body
            .chunks(4_096)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>();
        Body::from_stream(futures_util::stream::iter(chunks))
    } else {
        response = response.header("content-length", reply.body.len());
        Body::from(reply.body)
    };
    response
        .body(body)
        .expect("the owned profile response must be valid")
}

#[derive(Clone, Default)]
pub(super) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut capture = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("the log capture lock failed"))?;
        if bytes.len() > 1_048_576_usize.saturating_sub(capture.len()) {
            return Err(std::io::Error::other("the log capture exceeded its limit"));
        }
        capture.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        hito_testkit::logs::anchor_callsite_interest();
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter("off,hito_metadata_musicbrainz=trace")
                .with_ansi(false)
                .without_time()
                .json()
                .with_writer(move || capture.clone())
                .finish(),
        )
    }

    pub async fn profile(
        &self,
        client: &MusicBrainzClient,
        artist_id: String,
    ) -> Result<CatalogArtistProfile, Error> {
        client
            .artist_profile(ArtistProfileRequest { artist_id })
            .with_subscriber(self.dispatch())
            .await
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("the log capture must lock").clone())
            .expect("the captured workflow logs must be UTF-8")
    }
}
