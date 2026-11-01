use crate::Manifest;
use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::get,
};

pub fn with_manifest<S>(routes: Router<S>, manifest: Manifest, body_limit: usize) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    routes
        .route(
            "/v1/manifest",
            get(move || {
                let manifest = manifest.clone();
                async move {
                    let mut response: Response = Json(manifest).into_response();
                    response
                        .headers_mut()
                        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
                    response
                }
            }),
        )
        .layer(DefaultBodyLimit::max(body_limit))
}
