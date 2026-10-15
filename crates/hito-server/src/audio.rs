use crate::error::ApiResult;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use hito_core::{AudioFile, Error};
use std::io::SeekFrom;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

pub async fn response(audio: AudioFile, request: Request<Body>) -> ApiResult<Response> {
    let mut start = 0;
    let mut length = audio.length;
    let mut status = StatusCode::OK;
    if request.method() == Method::GET
        && !request.headers().contains_key(header::IF_RANGE)
        && let Some(value) = request.headers().get(header::RANGE)
    {
        let range = value
            .to_str()
            .ok()
            .and_then(|value| single_range(value, audio.length));
        let Some(range) = range else {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{}", audio.length))
                .header(header::CACHE_CONTROL, "private, no-store")
                .body(Body::empty())
                .unwrap());
        };
        start = *range.start();
        length = range.end() - start + 1;
        status = StatusCode::PARTIAL_CONTENT;
    }
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, audio.content_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store");
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, start + length - 1, audio.length),
        );
    }
    if request.method() == Method::HEAD {
        return Ok(builder.body(Body::empty()).unwrap());
    }
    let mut file = tokio::fs::File::from_std(audio.file);
    file.seek(SeekFrom::Start(start))
        .await
        .map_err(|_| Error::Internal("Audio could not be read".into()))?;
    let stream = ReaderStream::with_capacity(file.take(length), 65536);
    Ok(builder.body(Body::from_stream(stream)).unwrap())
}

fn single_range(value: &str, length: u64) -> Option<std::ops::RangeInclusive<u64>> {
    if value.len() > 1024 || length == 0 {
        return None;
    }
    let (unit, value) = value.trim().split_once('=')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (first, last) = value.split_once('-')?;
    if first.is_empty() {
        let suffix = decimal(last)?;
        return (suffix > 0).then(|| length.saturating_sub(suffix)..=length - 1);
    }
    let first = decimal(first)?;
    let last = if last.is_empty() {
        length - 1
    } else {
        decimal(last)?.min(length - 1)
    };
    (first <= last && first < length).then_some(first..=last)
}

fn decimal(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().unwrap_or(u64::MAX))
}
