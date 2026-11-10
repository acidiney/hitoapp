use super::{RemoteError, RemoteResult};
use reqwest::{Response, header::HeaderMap};
use serde::de::DeserializeOwned;
use std::time::Duration;
use zeroize::Zeroizing;

pub(super) async fn decode<T: DeserializeOwned>(
    mut response: Response,
    limit: usize,
) -> RemoteResult<T> {
    let headers = response.headers();
    match response.status().as_u16() {
        200 => (),
        401 => return Err(RemoteError::Unauthorized),
        423 => return Err(RemoteError::Locked),
        403 => return Err(RemoteError::Forbidden),
        409 => return Err(RemoteError::Conflict),
        429 => {
            return Err(RemoteError::RateLimited {
                retry_after: one(headers, "retry-after")?
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|seconds| Duration::from_secs(seconds.min(86_400))),
            });
        }
        500..=599 => return Err(RemoteError::Unavailable),
        _ => return Err(RemoteError::InvalidReply),
    }
    if headers.len() > 64
        || headers
            .iter()
            .map(|(key, value)| key.as_str().len() + value.len())
            .sum::<usize>()
            > 16_384
        || one(headers, "content-encoding")?
            .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"))
        || one(headers, "content-type")?
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
    serde_json::from_slice(&body).map_err(|_| RemoteError::InvalidReply)
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
