use crate::error;
use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Query, Request, rejection::JsonRejection},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;

pub struct JsonBody<T>(pub T);
pub struct QueryParams<T>(pub T);

pub struct InputError {
    status: StatusCode,
    kind: &'static str,
    message: &'static str,
}

impl<S, T> FromRequest<S> for JsonBody<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = InputError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| {
                let status = rejection.status();
                let (kind, message) = match rejection {
                    JsonRejection::JsonDataError(_) => (
                        "json_data",
                        "Request JSON does not match the expected fields",
                    ),
                    JsonRejection::JsonSyntaxError(_) => {
                        ("json_syntax", "Request body is not valid JSON")
                    }
                    JsonRejection::MissingJsonContentType(_) => {
                        ("json_content_type", "Use the application/json content type")
                    }
                    JsonRejection::BytesRejection(_) => {
                        ("json_body", "Request body could not be read")
                    }
                    _ => ("json", "JSON request could not be read"),
                };
                InputError {
                    status,
                    kind,
                    message,
                }
            })
    }
}

impl<S, T> FromRequestParts<S> for QueryParams<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = InputError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|rejection| InputError {
                status: rejection.status(),
                kind: "query",
                message: "Request query does not match the expected fields",
            })
    }
}

impl IntoResponse for InputError {
    fn into_response(self) -> Response {
        tracing::debug!(
            event = "request_rejected",
            error_kind = self.kind,
            status = self.status.as_u16(),
            "Request input rejected"
        );
        error::response(self.status, self.message)
    }
}
