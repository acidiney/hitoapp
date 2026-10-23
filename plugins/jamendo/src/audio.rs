mod headers;
mod stream;

use crate::{catalog::Catalog, error::Error, transport::Transport};
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use std::{sync::Arc, time::Instant};
use tokio::sync::Semaphore;

pub(crate) struct Relay {
    catalog: Arc<Catalog>,
    transport: Arc<Transport>,
    slots: Arc<Semaphore>,
}

impl Relay {
    pub(crate) fn new(catalog: Arc<Catalog>, transport: Arc<Transport>) -> Self {
        Self {
            catalog,
            transport,
            slots: Arc::new(Semaphore::new(16)),
        }
    }

    pub(crate) async fn open(
        &self,
        source_id: &str,
        method: Method,
        request_headers: &HeaderMap,
    ) -> Result<Response, Error> {
        let started = Instant::now();
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let selection = headers::Selection::read(request_headers, method == Method::HEAD)?;
        let endpoint = self.catalog.stream(source_id).await?;
        let upstream = self
            .transport
            .audio(
                endpoint,
                method.clone(),
                selection.map(|range| range.header()),
            )
            .await?;
        if upstream.status() == StatusCode::RANGE_NOT_SATISFIABLE && selection.is_some() {
            let total = headers::unsatisfied(&upstream)?;
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [
                    ("content-range", format!("bytes */{total}")),
                    ("cache-control", "private, no-store".to_owned()),
                ],
            )
                .into_response());
        }
        let representation = headers::verify(&upstream, selection)?;
        let status = upstream.status();
        let mut response = Response::builder()
            .status(status)
            .header("content-type", representation.content_type)
            .header("content-length", representation.length)
            .header("accept-ranges", "bytes")
            .header("cache-control", "private, no-store");
        if let Some(range) = representation.content_range {
            response = response.header("content-range", range);
        }
        let body = if method == Method::HEAD {
            drop(upstream);
            drop(permit);
            Body::empty()
        } else {
            Body::from_stream(stream::audio(upstream, representation.length, permit))
        };
        tracing::debug!(
            event = "jamendo_audio_open_complete",
            outcome = "ready",
            status = status.as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        response.body(body).map_err(|_| Error::Unavailable)
    }
}
