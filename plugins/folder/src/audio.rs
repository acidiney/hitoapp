use crate::{extract::QueryParams, http::HttpState};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioRequest {
    source_id: String,
}

pub(crate) async fn audio(
    State(state): State<Arc<HttpState>>,
    QueryParams(request): QueryParams<AudioRequest>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if request.source_id.len() > 256 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(permit) = state.audio_slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let catalog = state.catalog.clone();
    let source = request.source_id;
    let Ok(file_permit) = state.file_slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let opened = tokio::task::spawn_blocking(move || {
        let _file_permit = file_permit;
        catalog
            .open_source(&source, deadline)
            .map(|opened| opened.map(|opened| (opened.file, opened.length, opened.mime, permit)))
    })
    .await;
    let (file, total, mime, permit) = match opened {
        Ok(Ok(Some(opened))) => opened,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let mut first = 0;
    let mut length = total;
    let mut status = StatusCode::OK;
    let mut response_headers = HeaderMap::new();
    response_headers.insert("content-type", mime.parse().unwrap());
    response_headers.insert("accept-ranges", "bytes".parse().unwrap());
    response_headers.insert("cache-control", "private, no-store".parse().unwrap());
    response_headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    if method != Method::HEAD && !headers.contains_key("if-range") {
        let ranges: Vec<_> = headers.get_all("range").iter().collect();
        let selected = if ranges.len() > 1 {
            Err(())
        } else if let Some(range) = ranges.first() {
            range
                .to_str()
                .map_err(|_| ())
                .and_then(|range| range_selection(range, total))
        } else {
            Ok(None)
        };
        match selected {
            Ok(Some((start, end))) => {
                first = start;
                length = end - start + 1;
                status = StatusCode::PARTIAL_CONTENT;
                response_headers.insert(
                    "content-range",
                    format!("bytes {start}-{end}/{total}").parse().unwrap(),
                );
            }
            Ok(None) => {}
            Err(()) => {
                response_headers
                    .insert("content-range", format!("bytes */{total}").parse().unwrap());
                return (StatusCode::RANGE_NOT_SATISFIABLE, response_headers).into_response();
            }
        }
    }
    response_headers.insert("content-length", length.to_string().parse().unwrap());
    if method == Method::HEAD {
        return (status, response_headers, Body::empty()).into_response();
    }
    (
        status,
        response_headers,
        crate::stream::body(file, first, length, permit),
    )
        .into_response()
}

fn range_selection(range: &str, total: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some((unit, range)) = range.split_once('=') else {
        return Err(());
    };
    if !unit.eq_ignore_ascii_case("bytes") {
        return Ok(None);
    }
    let (first, last) = range.split_once('-').ok_or(())?;
    let number = |value: &str| -> Result<u64, ()> {
        if value.is_empty() || !value.bytes().all(|value| value.is_ascii_digit()) {
            return Err(());
        }
        value.parse().map_err(|_| ())
    };
    if first.is_empty() {
        let suffix = number(last)?;
        if suffix == 0 || total == 0 {
            return Err(());
        }
        return Ok(Some((total.saturating_sub(suffix), total - 1)));
    }
    let first = number(first)?;
    let last = if last.is_empty() {
        total.saturating_sub(1)
    } else {
        number(last)?.min(total.saturating_sub(1))
    };
    if first >= total || last < first {
        return Err(());
    }
    Ok(Some((first, last)))
}
