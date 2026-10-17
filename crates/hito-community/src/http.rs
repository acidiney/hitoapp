use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Request, State},
    http::{StatusCode, request::Parts},
    middleware,
    routing::post,
};
use serde::de::DeserializeOwned;

use crate::{
    Error, Service,
    admission::{Administrator, Reporter},
    wire::{
        OutcomeReceipt, ProvisionReporter, RankCandidates, Ranking, ReporterCredential,
        SubmitOutcome,
    },
};

pub fn router(service: Service) -> Router {
    Router::new()
        .route("/v1/admin/reporters", post(provision))
        .route("/v1/outcomes", post(submit))
        .route("/v1/rankings", post(rank))
        .layer(DefaultBodyLimit::max(16_384))
        .layer(middleware::from_fn(crate::telemetry::request))
        .with_state(service)
}

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

struct SafeJson<T>(T);

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
