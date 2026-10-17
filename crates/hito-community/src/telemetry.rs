use std::time::Instant;

use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use tracing::Instrument;
use uuid::Uuid;

pub(crate) async fn request(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched");
    let method = match request.method().as_str() {
        "POST" => "POST",
        _ => "other",
    };
    let span = tracing::debug_span!("community_http_request", request_id, route, method);
    let started = Instant::now();
    let mut response = next.run(request).instrument(span.clone()).await;
    span.in_scope(|| {
        tracing::debug!(
            event = "community_http_response",
            status = response.status().as_u16(),
            duration_ms = started.elapsed().as_millis() as u64
        );
    });
    response
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}
