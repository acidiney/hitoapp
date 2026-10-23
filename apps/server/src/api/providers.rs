use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{AdoptProviderRecording, ProviderSearch, Recording, Services};
use hito_plugin_sdk::SearchPage;

#[utoipa::path(post,path="/api/v1/plugins/{id}/search",operation_id="search_provider",
    params(("id"=String,Path)),request_body=ProviderSearch,security(("session"=[]),("bearer"=[])),responses(
        (status=200,body=SearchPage),(status=401,body=ErrorResponse),(status=404,body=ErrorResponse),
        (status=409,body=ErrorResponse),(status=422,body=ErrorResponse),(status=502,body=ErrorResponse),
))]
pub async fn search(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<ProviderSearch>,
) -> ApiResult<Json<SearchPage>> {
    Ok(Json(
        services
            .catalog
            .search_provider(&actor, &id, request)
            .await?,
    ))
}

#[utoipa::path(post,path="/api/v1/plugins/{id}/recordings",operation_id="adopt_provider_recording",
    params(("id"=String,Path)),request_body=AdoptProviderRecording,security(("session"=[]),("bearer"=[])),responses(
        (status=200,body=Recording),(status=401,body=ErrorResponse),(status=404,body=ErrorResponse),
        (status=409,body=ErrorResponse),(status=422,body=ErrorResponse),(status=502,body=ErrorResponse),
))]
pub async fn adopt(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<AdoptProviderRecording>,
) -> ApiResult<Json<Recording>> {
    Ok(Json(
        services
            .catalog
            .adopt_provider(&actor, &id, request)
            .await?,
    ))
}
