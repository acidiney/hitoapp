use super::{RemoteError, RemoteResult};
use reqwest::{Response, header::HeaderMap};
use serde::de::DeserializeOwned;
use std::time::Duration;
use zeroize::Zeroizing;

pub(super) async fn decode<T: DeserializeOwned>(
    response: Response,
    limit: usize,
) -> RemoteResult<T> {
    decode_counted(response, limit)
        .await
        .map(|(value, _)| value)
}

pub(super) async fn decode_counted<T: DeserializeOwned>(
    mut response: Response,
    limit: usize,
) -> RemoteResult<(T, usize)> {
    status(&response, 200)?;
    let headers = response.headers();
    bounded_headers(headers)?;
    if one(headers, "content-type")?
        .and_then(|value| value.split(';').next())
        .is_none_or(|mime| !mime.trim().eq_ignore_ascii_case("application/json"))
        || response
            .content_length()
            .is_some_and(|length| length > limit as u64)
    {
        return Err(RemoteError::InvalidReply);
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RemoteError::Unavailable)?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(RemoteError::InvalidReply);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map(|value| (value, body.len()))
        .map_err(|_| RemoteError::InvalidReply)
}

pub(super) async fn empty(mut response: Response) -> RemoteResult<()> {
    status(&response, 204)?;
    let headers = response.headers();
    bounded_headers(headers)?;
    if one(headers, "transfer-encoding")?.is_some()
        || one(headers, "content-length")?.is_some_and(|value| value != "0")
    {
        return Err(RemoteError::InvalidReply);
    }
    one(headers, "content-type")?;
    if response
        .chunk()
        .await
        .map_err(|_| RemoteError::Unavailable)?
        .is_some()
    {
        return Err(RemoteError::InvalidReply);
    }
    Ok(())
}

fn status(response: &Response, expected: u16) -> RemoteResult<()> {
    match response.status().as_u16() {
        value if value == expected => Ok(()),
        401 => Err(RemoteError::Unauthorized),
        423 => Err(RemoteError::Locked),
        403 => Err(RemoteError::Forbidden),
        404 => Err(RemoteError::NotFound),
        409 => Err(RemoteError::Conflict),
        429 => Err(RemoteError::RateLimited {
            retry_after: one(response.headers(), "retry-after")?
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| Duration::from_secs(seconds.min(86_400))),
        }),
        500..=599 => Err(RemoteError::Unavailable),
        _ => Err(RemoteError::InvalidReply),
    }
}

fn bounded_headers(headers: &HeaderMap) -> RemoteResult<()> {
    if headers.len() > 64
        || headers
            .iter()
            .map(|(key, value)| key.as_str().len() + value.len())
            .sum::<usize>()
            > 16_384
        || one(headers, "content-encoding")?
            .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"))
    {
        return Err(RemoteError::InvalidReply);
    }
    one(headers, "content-length")?;
    Ok(())
}

fn one<'a>(headers: &'a HeaderMap, name: &'static str) -> RemoteResult<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    let first = values
        .next()
        .map(|value| value.to_str().map_err(|_| RemoteError::InvalidReply))
        .transpose()?;
    if values.next().is_some() {
        return Err(RemoteError::InvalidReply);
    }
    Ok(first)
}
