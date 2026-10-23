use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::OptionalJsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{Services, Source};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceRefreshPurpose {
    #[default]
    Manual,
    Playback,
}

#[derive(Default, Deserialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RefreshSources {
    pub purpose: SourceRefreshPurpose,
}

#[utoipa::path(post, path = "/api/v1/recordings/{id}/sources/refresh", params(("id" = String, Path)),
    request_body(content = Option<RefreshSources>, content_type = "application/json"),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [Source]), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 502, body = ErrorResponse),
))]
pub async fn refresh_sources(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    OptionalJsonBody(request): OptionalJsonBody<RefreshSources>,
) -> ApiResult<Json<Vec<Source>>> {
    let sources = match request.unwrap_or_default().purpose {
        SourceRefreshPurpose::Manual => services.catalog.refresh_sources(&actor, &id).await?,
        SourceRefreshPurpose::Playback => {
            services
                .catalog
                .refresh_playback_sources_until(&actor, &id, std::future::pending())
                .await?;
            services.catalog.sources(&actor, &id).await?
        }
    };
    Ok(Json(sources))
}
