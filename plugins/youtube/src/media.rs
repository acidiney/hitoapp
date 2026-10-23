mod headers;
mod range;
mod stream;
mod transport;
pub(crate) use transport::Client;

use crate::{Error, recording::Catalog};
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode},
    response::Response,
};
use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) struct Relay {
    catalog: Arc<Catalog>,
    client: Result<Client, Error>,
    slots: Arc<Semaphore>,
    admission: Mutex<()>,
    connections: Mutex<Vec<Weak<Mutex<Option<stream::Connection>>>>>,
    tasks: TaskTracker,
    stopped: CancellationToken,
}

impl Relay {
    pub(crate) fn new(catalog: Arc<Catalog>, client: Result<Client, Error>) -> Self {
        Self {
            catalog,
            client,
            slots: Arc::new(Semaphore::new(4)),
            admission: Mutex::new(()),
            connections: Mutex::new(Vec::new()),
            tasks: TaskTracker::new(),
            stopped: CancellationToken::new(),
        }
    }

    pub(crate) async fn open(
        self: &Arc<Self>,
        source: &str,
        method: Method,
        headers: HeaderMap,
    ) -> Result<Response, Error> {
        let started = Instant::now();
        let (permit, _opening) = {
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.stopped.is_cancelled() {
                return Err(Error::Unavailable);
            }
            let permit = self
                .slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::Busy)?;
            (permit, self.tasks.token())
        };
        let mut permit = Some(permit);
        let operation = async {
            let descriptor = self.catalog.source(source).await?;
            let head = method == Method::HEAD;
            let selected = match range::read(&headers, head, descriptor.length) {
                Ok(selected) => selected,
                Err(()) => return Ok(unsatisfiable(descriptor.length)),
            };
            let client = self.client.as_ref().map_err(|error| *error)?;
            let upstream = client
                .request(
                    descriptor.endpoint()?,
                    if head { Method::HEAD } else { Method::GET },
                    selected,
                )
                .await?;
            let length = headers::verify(&upstream, &descriptor, selected)?;
            let mut response = Response::builder()
                .status(if selected.is_some() {
                    StatusCode::PARTIAL_CONTENT
                } else {
                    StatusCode::OK
                })
                .header("content-type", "audio/mp4")
                .header("content-length", length)
                .header("accept-ranges", "bytes")
                .header("cache-control", "private, no-store")
                .header("x-content-type-options", "nosniff");
            if let Some(selected) = selected {
                response = response.header(
                    "content-range",
                    format!(
                        "bytes {}-{}/{}",
                        selected.first, selected.last, descriptor.length
                    ),
                );
            }
            if head {
                return response.body(Body::empty()).map_err(|_| Error::Unavailable);
            }
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.stopped.is_cancelled() {
                return Err(Error::Canceled);
            }
            let (body, connection) = stream::Audio::new(
                upstream.into_body(),
                length,
                permit.take().ok_or(Error::Unavailable)?,
                self.tasks.token(),
                self.stopped.clone(),
            );
            let mut connections = self
                .connections
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            connections.retain(|entry| entry.strong_count() > 0);
            connections.push(Arc::downgrade(&connection));
            response
                .body(Body::from_stream(body))
                .map_err(|_| Error::Unavailable)
        };
        let result = tokio::select! {
            biased;
            _ = self.stopped.cancelled() => Err(Error::Canceled),
            result = tokio::time::timeout(Duration::from_secs(8), operation) => result.unwrap_or(Err(Error::Deadline)),
        };
        tracing::debug!(
            event = "youtube_audio_open_complete",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.outcome(), |_| "complete"),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        result
    }

    pub(crate) async fn shutdown(&self) {
        let started = Instant::now();
        {
            let _gate = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.tasks.close();
            self.stopped.cancel();
            let mut connections = self
                .connections
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for connection in connections.drain(..).filter_map(|entry| entry.upgrade()) {
                *connection.lock().unwrap_or_else(|error| error.into_inner()) = None;
            }
        }
        self.tasks.wait().await;
        tracing::info!(
            event = "youtube_audio_shutdown_complete",
            outcome = "complete",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
    }
}

fn unsatisfiable(length: u64) -> Response {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header("content-range", format!("bytes */{length}"))
        .header("content-length", 0)
        .header("cache-control", "private, no-store")
        .body(Body::empty())
        .unwrap()
}
