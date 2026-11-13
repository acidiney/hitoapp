use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    ApplyArtistIdentityToCredit, ArtistIdentityChoiceReceipt, ArtistIdentityCreditReceipt,
    ChooseCreditMemberIdentity, ChooseRecordingArtistIdentity, CreditMemberIdentityChoiceReceipt,
    RecordingArtistIdentityChoice, RecordingCreditMemberIdentityChoice,
    ResetRecordingArtistIdentity, Services,
};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artist-identity", operation_id = "recording_artist_identity_choice",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingArtistIdentityChoice), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_identity_choice", request_id = %Uuid::new_v4()))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<RecordingArtistIdentityChoice>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .recording_artist_identity_choice(&actor, &id)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity choice HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(put, path = "/api/v1/recordings/{id}/artist-identity", operation_id = "choose_recording_artist_identity",
    params(("id" = String, Path)), request_body = ChooseRecordingArtistIdentity,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityChoiceReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_identity_choose", request_id = %Uuid::new_v4()))]
pub async fn choose(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ChooseRecordingArtistIdentity>,
) -> ApiResult<Json<ArtistIdentityChoiceReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .choose_recording_artist_identity(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity selection HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/artist-identity/reset", operation_id = "reset_recording_artist_identity",
    params(("id" = String, Path)), request_body = ResetRecordingArtistIdentity,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityChoiceReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_identity_reset", request_id = %Uuid::new_v4()))]
pub async fn reset(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ResetRecordingArtistIdentity>,
) -> ApiResult<Json<ArtistIdentityChoiceReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .reset_recording_artist_identity(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity reset HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artists/{id}/artist-identity", operation_id = "apply_artist_identity_to_credit",
    params(("id" = String, Path)), request_body = ApplyArtistIdentityToCredit,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityCreditReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_identity_credit", request_id = %Uuid::new_v4()))]
pub async fn apply_to_credit(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ApplyArtistIdentityToCredit>,
) -> ApiResult<Json<ArtistIdentityCreditReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .apply_artist_identity_to_credit(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity credit HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/credit-members/{artist}/artist-identity", operation_id = "recording_credit_member_identity_choice",
    params(("id" = String, Path), ("artist" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = RecordingCreditMemberIdentityChoice), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_credit_member_identity_read", request_id = %Uuid::new_v4()))]
pub async fn member_read(
    State(services): State<Services>,
    Account(actor): Account,
    Path((recording, artist)): Path<(String, String)>,
) -> ApiResult<Json<RecordingCreditMemberIdentityChoice>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &recording, &artist)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "returned"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Credit member identity HTTP read completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(put, path = "/api/v1/recordings/{id}/credit-members/{artist}/artist-identity", operation_id = "choose_credit_member_identity",
    params(("id" = String, Path), ("artist" = String, Path)), request_body = ChooseCreditMemberIdentity,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = CreditMemberIdentityChoiceReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_credit_member_identity_choose", request_id = %Uuid::new_v4()))]
pub async fn member_choose(
    State(services): State<Services>,
    Account(actor): Account,
    Path((recording, artist)): Path<(String, String)>,
    JsonBody(request): JsonBody<ChooseCreditMemberIdentity>,
) -> ApiResult<Json<CreditMemberIdentityChoiceReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .choose_credit_member_identity(&actor, &recording, &artist, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Credit member identity HTTP choice completed"
    );
    Ok(Json(result?))
}
