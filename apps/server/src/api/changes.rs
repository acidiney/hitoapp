use crate::{auth::Account, error::ApiResult, error::ErrorResponse};
use axum::{
    extract::State,
    http::{HeaderValue, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::stream;
use hito_core::{ChangeNotice, ChangeStream, Services};
use std::{convert::Infallible, time::Duration};

#[utoipa::path(get, path = "/api/v1/changes/events", operation_id = "change_events",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ChangeNotice, content_type = "text/event-stream", description = "Each SSE data frame contains a ChangeNotice. The first frame is a resync notice with reason connected; a resync notice with reason lagged means notices were dropped. The stream ends when the session ends or is locked"),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse), (status = 429, body = ErrorResponse),
))]
pub async fn events(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Response> {
    let changes = services.changes.subscribe(&actor).await?;
    let mut response = Sse::new(notices(changes))
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

fn notices(
    changes: ChangeStream,
) -> impl futures_util::Stream<Item = Result<Event, Infallible>> + Send {
    stream::unfold(changes, |mut changes| async move {
        let notice = changes.next().await?;
        match Event::default().json_data(&notice) {
            Ok(event) => Some((Ok(event), changes)),
            Err(_) => {
                tracing::error!(
                    event = "change_notice_encoding_failed",
                    kind = notice.kind(),
                    "Change notice could not be encoded"
                );
                None
            }
        }
    })
}
