use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::StatusCode,
};
use uuid::Uuid;

use crate::{
    Error, Service,
    admission::{Administrator, Reporter},
    corrections::types::{
        CorrectionPage, CorrectionReceipt, ListCorrections, ReviewCorrection, ReviewReceipt,
        SuggestCorrection,
    },
    http::SafeJson,
    wire::Rejection,
};

#[utoipa::path(post, path = "/v1/corrections", operation_id = "submit_correction",
    request_body(content = SuggestCorrection, description = "Strict JSON body, at most 16 KiB. The source pair is unordered."),
    security(("reporterBearer" = [])), responses(
        (status = 201, body = CorrectionReceipt), (status = 200, body = CorrectionReceipt),
        (status = 400, body = Rejection), (status = 401, body = Rejection),
        (status = 409, body = Rejection), (status = 413, body = Rejection),
        (status = 415, body = Rejection), (status = 422, body = Rejection),
        (status = 429, body = Rejection, headers(("Retry-After" = String, description = "60 seconds; development defaults allow 10 new suggestions per reporter per rolling minute, 32 pending per reporter, and 1024 pending overall."))),
        (status = 503, body = Rejection),
    ),
)]
pub(crate) async fn submit(
    State(service): State<Service>,
    reporter: Reporter,
    SafeJson(request): SafeJson<SuggestCorrection>,
) -> Result<(StatusCode, Json<CorrectionReceipt>), Error> {
    let (created, receipt) = service.corrections.submit(reporter, request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(receipt),
    ))
}

#[utoipa::path(get, path = "/v1/admin/corrections", operation_id = "list_corrections", params(ListCorrections),
    security(("administratorBearer" = [])), responses(
        (status = 200, body = CorrectionPage), (status = 400, body = Rejection),
        (status = 401, body = Rejection), (status = 503, body = Rejection),
    ),
)]
pub(crate) async fn list(
    State(service): State<Service>,
    administrator: Administrator,
    query: Result<Query<ListCorrections>, QueryRejection>,
) -> Result<Json<CorrectionPage>, Error> {
    let Query(query) = query.map_err(|_| Error::InvalidInput)?;
    Ok(Json(service.corrections.list(administrator, query).await?))
}

#[utoipa::path(post, path = "/v1/admin/corrections/{correction_id}/review", operation_id = "review_correction",
    params(("correction_id" = Uuid, Path, description = "Version-4 correction identifier.")),
    request_body(content = ReviewCorrection, description = "Strict JSON body, at most 16 KiB. Every accepted new request advances the expected revision, including repeated decisions."),
    security(("administratorBearer" = [])), responses(
        (status = 201, body = ReviewReceipt), (status = 200, body = ReviewReceipt),
        (status = 400, body = Rejection), (status = 401, body = Rejection),
        (status = 404, body = Rejection), (status = 409, body = Rejection),
        (status = 413, body = Rejection), (status = 415, body = Rejection),
        (status = 422, body = Rejection), (status = 503, body = Rejection),
    ),
)]
pub(crate) async fn review(
    State(service): State<Service>,
    administrator: Administrator,
    correction_id: Result<Path<Uuid>, PathRejection>,
    SafeJson(request): SafeJson<ReviewCorrection>,
) -> Result<(StatusCode, Json<ReviewReceipt>), Error> {
    let Path(correction_id) = correction_id.map_err(|_| Error::InvalidInput)?;
    let (created, receipt) = service
        .corrections
        .review(administrator, correction_id, request)
        .await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(receipt),
    ))
}
