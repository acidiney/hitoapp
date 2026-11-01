use axum::{
    Extension, Json, Router,
    extract::{
        DefaultBodyLimit, FromRequest, FromRequestParts, Path, Request, State,
        rejection::PathRejection,
    },
    http::{StatusCode, request::Parts},
    middleware,
};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    Error, Service,
    admission::{Administrator, Reporter},
    wire::{
        OutcomeReceipt, ProvisionReporter, RankCandidates, Ranking, Rejection, ReporterCredential,
        SubmitOutcome,
    },
};

pub fn router(service: Service) -> Router {
    let (router, document) = OpenApiRouter::with_openapi(crate::contract::base_document())
        .routes(routes!(provision))
        .routes(routes!(revoke))
        .routes(routes!(submit))
        .routes(routes!(rank))
        .routes(routes!(crate::correction_http::submit))
        .routes(routes!(crate::correction_http::list))
        .routes(routes!(crate::correction_http::review))
        .routes(routes!(crate::contract::serve))
        .split_for_parts();
    router
        .layer(Extension(Arc::new(document)))
        .layer(DefaultBodyLimit::max(16_384))
        .layer(middleware::from_fn(crate::telemetry::request))
        .with_state(service)
}

#[utoipa::path(post, path = "/v1/admin/reporters",
    request_body(content = ProvisionReporter, description = "Strict JSON body, at most 16 KiB."),
    security(("administratorBearer" = [])), responses(
        (status = 201, body = ReporterCredential),
        (status = 400, body = Rejection), (status = 401, body = Rejection),
        (status = 413, body = Rejection), (status = 415, body = Rejection),
        (status = 422, body = Rejection), (status = 503, body = Rejection),
    ),
)]
async fn provision(
    State(service): State<Service>,
    administrator: Administrator,
    SafeJson(_request): SafeJson<ProvisionReporter>,
) -> Result<(StatusCode, Json<ReporterCredential>), Error> {
    Ok((
        StatusCode::CREATED,
        Json(service.admission.provision(administrator).await?),
    ))
}

#[utoipa::path(delete, path = "/v1/admin/reporters/{reporter_id}",
    params(("reporter_id" = Uuid, Path, description = "Reporter pseudonym to revoke.")),
    security(("administratorBearer" = [])), responses(
        (status = 204, description = "Reporter absent and its outcomes erased."),
        (status = 400, body = Rejection), (status = 401, body = Rejection),
        (status = 503, body = Rejection),
    ),
)]
async fn revoke(
    State(service): State<Service>,
    administrator: Administrator,
    reporter_id: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, Error> {
    let Path(reporter_id) = reporter_id.map_err(|_| Error::InvalidInput)?;
    service.admission.revoke(administrator, reporter_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/v1/outcomes",
    request_body(content = SubmitOutcome, description = "Strict JSON body, at most 16 KiB."),
    security(("reporterBearer" = [])), responses(
        (status = 201, body = OutcomeReceipt), (status = 200, body = OutcomeReceipt),
        (status = 400, body = Rejection), (status = 401, body = Rejection),
        (status = 409, body = Rejection), (status = 413, body = Rejection),
        (status = 429, body = Rejection, headers(("Retry-After" = String, description = "60 seconds; at most 300 new accepted outcomes per reporter in a rolling 60-second window."))),
        (status = 415, body = Rejection), (status = 422, body = Rejection),
        (status = 503, body = Rejection),
    ),
)]
async fn submit(
    State(service): State<Service>,
    reporter: Reporter,
    SafeJson(request): SafeJson<SubmitOutcome>,
) -> Result<(StatusCode, Json<OutcomeReceipt>), Error> {
    let (created, receipt) = service.outcomes.submit(reporter, request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(receipt),
    ))
}

#[utoipa::path(post, path = "/v1/rankings",
    request_body(content = RankCandidates, description = "Strict JSON body, at most 16 KiB."),
    responses(
        (status = 200, body = Ranking), (status = 400, body = Rejection),
        (status = 413, body = Rejection), (status = 415, body = Rejection),
        (status = 422, body = Rejection), (status = 503, body = Rejection),
    ),
)]
async fn rank(
    State(service): State<Service>,
    SafeJson(request): SafeJson<RankCandidates>,
) -> Result<Json<Ranking>, Error> {
    Ok(Json(service.rankings.rank(request).await?))
}

impl FromRequestParts<Service> for Administrator {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &Service) -> Result<Self, Error> {
        state.admission.administrator(bearer(parts)?)
    }
}

impl FromRequestParts<Service> for Reporter {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &Service) -> Result<Self, Error> {
        state.admission.reporter(bearer(parts)?).await
    }
}

fn bearer(parts: &Parts) -> Result<&str, Error> {
    let mut values = parts.headers.get_all("authorization").iter();
    let value = values.next().ok_or(Error::Unauthorized)?;
    if values.next().is_some() {
        return Err(Error::Unauthorized);
    }
    let token = value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(Error::Unauthorized)?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Unauthorized);
    }
    Ok(token)
}

pub(crate) struct SafeJson<T>(pub(crate) T);

impl<T> FromRequest<Service> for SafeJson<T>
where
    T: DeserializeOwned,
{
    type Rejection = Error;

    async fn from_request(request: Request, state: &Service) -> Result<Self, Error> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| Error::Input(rejection.status()))
    }
}
