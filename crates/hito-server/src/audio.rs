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
            .filter(|value| value.len() <= 1024 && !value.contains(','))
            .and_then(|value| {
                let suffix = value
                    .strip_prefix("bytes=-")
                    .and_then(|length| length.parse::<u64>().ok());
                let value =
                    if audio.length > 0 && suffix.is_some_and(|length| length > audio.length) {
                        std::borrow::Cow::Owned(format!("bytes=0-{}", audio.length - 1))
                    } else {
                        std::borrow::Cow::Borrowed(value)
                    };
                http_range_header::parse_range_header(&value).ok()
            })
            .and_then(|ranges| ranges.validate(audio.length).ok())
            .and_then(|mut ranges| ranges.pop());
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
