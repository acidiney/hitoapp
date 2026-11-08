use super::range::{Selection, decimal};
use crate::{Error, recording::Descriptor};
use axum::http::{HeaderMap, Response, StatusCode};

pub(super) fn verify(
    response: &Response<super::transport::Body>,
    source: &Descriptor,
    selected: Option<Selection>,
) -> Result<u64, Error> {
    if !response.status().is_success() {
        return Err(Error::UpstreamRejected);
    }
    let headers = response.headers();
    if headers.len() > 32
        || headers.contains_key("transfer-encoding")
        || one(headers, "content-encoding")?
            .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::InvalidMedia);
    }
    let length = one(headers, "content-length")?
        .and_then(decimal)
        .ok_or(Error::InvalidMedia)?;
    if selected.is_none()
        && !one(headers, "accept-ranges")?.is_some_and(|value| value.eq_ignore_ascii_case("bytes"))
    {
        return Err(Error::InvalidMedia);
    }
    let raw_range = one(headers, "content-range")?;
    match selected {
        None if response.status() == StatusCode::OK
            && raw_range.is_none()
            && length == source.length => {}
        Some(selection)
            if response.status() == StatusCode::PARTIAL_CONTENT && length == selection.length() =>
        {
            let raw = raw_range.ok_or(Error::InvalidMedia)?;
            if raw.len() > 128 {
                return Err(Error::InvalidMedia);
            }
            let (unit, value) = raw.split_once(' ').ok_or(Error::InvalidMedia)?;
            let (range, total) = value.split_once('/').ok_or(Error::InvalidMedia)?;
            let (first, last) = range.split_once('-').ok_or(Error::InvalidMedia)?;
            if !unit.eq_ignore_ascii_case("bytes")
                || decimal(first) != Some(selection.first)
                || decimal(last) != Some(selection.last)
                || decimal(total) != Some(source.length)
            {
                return Err(Error::InvalidMedia);
            }
        }
        _ => return Err(Error::InvalidMedia),
    }
    let mime = one(headers, "content-type")?.ok_or(Error::InvalidMedia)?;
    if mime.len() > 128
        || !matches!(
            mime.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "audio/mp4" | "audio/x-m4a"
        )
    {
        return Err(Error::InvalidMedia);
    }
    source.verify_validator(one(headers, "etag")?)?;
    Ok(length)
}

fn one<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<Option<&'a str>, Error> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|value| value.to_str().map_err(|_| Error::InvalidMedia))
        .transpose()?;
    if values.next().is_some() {
        return Err(Error::InvalidMedia);
    }
    Ok(value)
}
