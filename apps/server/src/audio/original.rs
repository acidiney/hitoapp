use crate::error::ApiResult;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use hito_core::{Actor, AudioRequest, ByteRange, OpenedAudio, PlaybackAccess};

pub(super) async fn response(
    playback: &PlaybackAccess,
    actor: &Actor,
    source: &str,
    request: Request<Body>,
) -> ApiResult<Response> {
    let head = request.method() == Method::HEAD;
    let parsed = if head || request.headers().contains_key(header::IF_RANGE) {
        Ok(None)
    } else {
        range(request.headers())
    };
    let invalid = parsed.is_err();
    let opened = playback
        .open_audio(
            actor,
            source,
            AudioRequest {
                head: head || invalid,
                range: parsed.unwrap_or(None),
            },
        )
        .await?;
    content(opened, invalid)
}

pub(super) fn content(opened: OpenedAudio, invalid: bool) -> ApiResult<Response> {
    let mut audio = match opened {
        OpenedAudio::Unsatisfiable { length } => return Ok(unsatisfiable(length)),
        OpenedAudio::Content(audio) if invalid => return Ok(unsatisfiable(audio.total_length)),
        OpenedAudio::Content(audio) => audio,
    };
    let mut builder = Response::builder()
        .header("x-hito-stream-quality", "original")
        .status(if audio.range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, audio.content_type)
        .header(
            header::ACCEPT_RANGES,
            if audio.seekable { "bytes" } else { "none" },
        )
        .header(header::CACHE_CONTROL, "private, no-store");
    if let Some(length) = audio.length {
        builder = builder.header(header::CONTENT_LENGTH, length);
    }
    if let Some(range) = audio.range {
        let total = audio
            .total_length
            .map_or_else(|| "*".into(), |total| total.to_string());
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{total}", range.start(), range.end()),
        );
    }
    let body = audio
        .body
        .take()
        .map(|stream| {
            Body::from_stream(futures_util::stream::try_unfold(
                stream,
                |mut stream| async move {
                    stream
                        .chunk()
                        .await
                        .map(|chunk| chunk.map(|bytes| (bytes, stream)))
                },
            ))
        })
        .unwrap_or_else(Body::empty);
    Ok(builder.body(body).unwrap())
}

fn unsatisfiable(length: Option<u64>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::CONTENT_LENGTH, 0);
    if let Some(length) = length {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes */{length}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn range(headers: &axum::http::HeaderMap) -> Result<Option<ByteRange>, ()> {
    let mut values = headers.get_all(header::RANGE).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let value = value.to_str().map_err(|_| ())?;
    if value.len() > 1024 {
        return Err(());
    }
    let (unit, value) = value.trim().split_once('=').ok_or(())?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return Ok(None);
    }
    let (first, last) = value.split_once('-').ok_or(())?;
    if first.is_empty() {
        let length = decimal(last).ok_or(())?;
        return (length > 0)
            .then_some(Some(ByteRange::Suffix { length }))
            .ok_or(());
    }
    let first = decimal(first).ok_or(())?;
    let last = if last.is_empty() {
        None
    } else {
        Some(decimal(last).ok_or(())?)
    };
    if last.is_some_and(|last| last < first) {
        return Err(());
    }
    Ok(Some(ByteRange::From { first, last }))
}
fn decimal(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().unwrap_or(u64::MAX))
}
