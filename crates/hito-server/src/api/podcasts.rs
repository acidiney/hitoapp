use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use hito_core::{
    PodcastEpisodePage, PodcastEpisodeQuery, PodcastRefreshReceipt, PodcastSubscription,
    PodcastSubscriptionReceipt, PodcastUnsubscribeReceipt, RefreshPodcast, Services,
    SubscribePodcast, UnsubscribePodcast,
};

#[utoipa::path(get, path = "/api/v1/podcasts", operation_id = "podcast_subscriptions",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [PodcastSubscription]), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn subscriptions(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<PodcastSubscription>>> {
    Ok(Json(services.podcasts.subscriptions(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/podcasts", operation_id = "subscribe_podcast",
    request_body = SubscribePodcast, security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = PodcastSubscriptionReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn subscribe(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<SubscribePodcast>,
) -> ApiResult<(StatusCode, Json<PodcastSubscriptionReceipt>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(services.podcasts.subscribe(&actor, request).await?),
    ))
}

#[utoipa::path(get, path = "/api/v1/podcasts/{id}/episodes", operation_id = "podcast_episodes",
    params(("id" = String, Path), ("limit" = Option<u16>, Query), ("after" = Option<String>, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PodcastEpisodePage), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn episodes(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(query): QueryParams<PodcastEpisodeQuery>,
) -> ApiResult<Json<PodcastEpisodePage>> {
    Ok(Json(services.podcasts.episodes(&actor, &id, query).await?))
}

#[utoipa::path(post, path = "/api/v1/podcasts/{id}/refresh", operation_id = "refresh_podcast",
    params(("id" = String, Path)), request_body = RefreshPodcast,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 202, body = PodcastRefreshReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn refresh(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<RefreshPodcast>,
) -> ApiResult<(StatusCode, Json<PodcastRefreshReceipt>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            services
                .podcasts
                .enqueue_refresh(&actor, &id, request)
                .await?,
        ),
    ))
}

#[utoipa::path(post, path = "/api/v1/podcasts/{id}/unsubscribe", operation_id = "unsubscribe_podcast",
    params(("id" = String, Path)), request_body = UnsubscribePodcast,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PodcastUnsubscribeReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
pub async fn unsubscribe(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<UnsubscribePodcast>,
) -> ApiResult<Json<PodcastUnsubscribeReceipt>> {
    Ok(Json(
        services.podcasts.unsubscribe(&actor, &id, request).await?,
    ))
}
