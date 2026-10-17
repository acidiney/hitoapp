use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

pub fn application_log(metadata: &tracing::Metadata<'_>) -> bool {
    let target = metadata.target();
    [
        "hito_server",
        "hito_core",
        "hito_playback",
        "hito_audio",
        "hito_desktop",
        "hito_plugin_sdk",
    ]
    .iter()
    .any(|prefix| {
        target == *prefix
            || target
                .strip_prefix(*prefix)
                .is_some_and(|suffix| suffix.starts_with("::"))
    })
}

pub async fn request(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched");
    let span = tracing::info_span!("http_request", request_id = %request_id, method = %request.method(), route);
    let started = Instant::now();
    let mut response = next.run(request).instrument(span.clone()).await;
    span.in_scope(|| {
        tracing::info!(
            event = "http_response",
            status = response.status().as_u16(),
            duration_ms = started.elapsed().as_millis() as u64,
            "Request completed"
        );
    });
    response
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());
    response
}
