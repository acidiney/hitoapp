use axum::{body::Body, extract::Request, http::StatusCode};
use std::time::Duration;

pub async fn buffer(request: Request) -> Result<Request, (StatusCode, &'static str)> {
    let (parts, body) = request.into_parts();
    let bytes =
        match tokio::time::timeout(Duration::from_secs(5), axum::body::to_bytes(body, 16384)).await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => {
                tracing::warn!(
                    event = "auth_body_rejected",
                    "Sign-in request exceeded its body limit"
                );
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Sign-in request is too large",
                ));
            }
            Err(_) => {
                tracing::warn!(
                    event = "auth_body_timed_out",
                    "Sign-in request body timed out"
                );
                return Err((StatusCode::REQUEST_TIMEOUT, "Sign-in request timed out"));
            }
        };
    Ok(Request::from_parts(parts, Body::from(bytes)))
}
