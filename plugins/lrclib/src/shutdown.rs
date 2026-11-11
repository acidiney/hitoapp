use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

#[derive(Clone)]
pub struct Shutdown {
    requested: tokio::sync::watch::Sender<bool>,
}

impl Shutdown {
    pub(crate) fn new() -> Self {
        let (requested, _) = tokio::sync::watch::channel(false);
        Self { requested }
    }

    pub fn cancel(&self) {
        self.requested.send_replace(true);
    }
}

pub(crate) async fn observe(
    State(shutdown): State<Shutdown>,
    request: Request,
    next: Next,
) -> Response {
    let mut requested = shutdown.requested.subscribe();
    if !*requested.borrow() {
        tokio::select! {
            biased;
            _ = requested.changed() => {},
            response = next.run(request) => return response,
        }
    }
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error":"The lyric provider service is stopping"})),
    )
        .into_response();
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
