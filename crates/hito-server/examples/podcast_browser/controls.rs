use super::fixture::PodcastAudioServer;
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, sync::Mutex};

#[derive(Serialize)]
struct FixtureStatus {
    kind: &'static str,
    feed_url: String,
    feed_requests: usize,
    enclosure_requests: usize,
    unexpected_requests: usize,
    upstream_closed: bool,
}

pub(super) struct OwnedFixture {
    server: PodcastAudioServer,
    closed: bool,
}

impl OwnedFixture {
    pub(super) async fn start(audio: Vec<u8>) -> Self {
        Self {
            server: PodcastAudioServer::start_with_ranges(audio).await,
            closed: false,
        }
    }

    pub(super) fn origin(&self) -> &str {
        &self.server.origin
    }

    fn status(&self) -> FixtureStatus {
        FixtureStatus {
            kind: "hito-owned-podcast-browser",
            feed_url: self.server.feed_url.clone(),
            feed_requests: self.server.feed_requests(),
            enclosure_requests: self.server.enclosure_requests(),
            unexpected_requests: self.server.unexpected(),
            upstream_closed: self.closed,
        }
    }

    pub(super) async fn stop(&mut self) -> bool {
        if self.closed {
            return true;
        }
        if !self.server.finish().await {
            return false;
        }
        let Some(address) = self.server.origin.strip_prefix("http://") else {
            return false;
        };
        self.closed = matches!(
            tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(address)).await,
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused
        );
        self.closed
    }
}

pub(super) fn routes(fixture: Arc<Mutex<OwnedFixture>>) -> Router {
    let status_fixture = fixture.clone();
    Router::new()
        .route("/__podcast_fixture", get(move || {
            let fixture = status_fixture.clone();
            async move {
                let started = Instant::now();
                let request_id = uuid::Uuid::new_v4();
                let status = fixture.lock().await.status();
                tracing::debug!(
                    target: "hito_server::podcast_browser_fixture",
                    event = "fixture_status", outcome = "read", %request_id,
                    feed_requests = status.feed_requests, enclosure_requests = status.enclosure_requests,
                    unexpected_requests = status.unexpected_requests,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Owned podcast fixture status read"
                );
                Json(status)
            }
        }))
        .route("/__podcast_fixture/stop", post(move |headers: HeaderMap| {
            let fixture = fixture.clone();
            async move {
                if headers.get("x-hito-request").is_none_or(|value| value != "1")
                    || headers.get("origin").is_none_or(|value| value != "http://localhost:8080")
                {
                    return StatusCode::FORBIDDEN.into_response();
                }
                stop_upstream(fixture).await
            }
        }))
}

async fn stop_upstream(fixture: Arc<Mutex<OwnedFixture>>) -> Response {
    let started = Instant::now();
    let request_id = uuid::Uuid::new_v4();
    let mut fixture = fixture.lock().await;
    let closed = fixture.stop().await;
    let status = fixture.status();
    tracing::debug!(
        target: "hito_server::podcast_browser_fixture",
        event = "fixture_upstream_stopped", outcome = if closed { "closed" } else { "not_closed" },
        %request_id, feed_requests = status.feed_requests, enclosure_requests = status.enclosure_requests,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Owned podcast upstream stop finished"
    );
    (
        if closed {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(status),
    )
        .into_response()
}
