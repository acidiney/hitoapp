use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
    source_resolution::{self, Resolutions},
};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use hito_core::{ResolveSource, Services, SourceResolution};

#[utoipa::path(post, path = "/api/v1/recordings/{id}/sources/resolve", operation_id = "resolve_source",
    params(("id" = String, Path)), request_body = ResolveSource,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SourceResolution), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
    (status = 502, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.user().id.as_str()))]
pub async fn resolve_source(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    owner: Option<Extension<Resolutions>>,
    JsonBody(request): JsonBody<ResolveSource>,
) -> ApiResult<Json<SourceResolution>> {
    Ok(Json(
        source_resolution::run(
            owner.map(|Extension(owner)| owner),
            move |cancelled| async move {
                services
                    .catalog
                    .resolve_source_until(&actor, &id, request, async {
                        let _ = cancelled.await;
                    })
                    .await
            },
        )
        .await?,
    ))
}
