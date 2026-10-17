use crate::error::Error;
use axum::http::{HeaderMap, StatusCode};

#[derive(Clone, Copy)]
pub(super) enum Selection {
    Bounded { first: u64, last: u64 },
    Open { first: u64 },
    Suffix { length: u64 },
}

pub(super) struct Representation {
    pub content_type: &'static str,
    pub length: u64,
    pub content_range: Option<String>,
}

impl Selection {
    pub(super) fn read(headers: &HeaderMap, head: bool) -> Result<Option<Self>, Error> {
        if head || headers.contains_key("if-range") {
            return Ok(None);
        }
        let raw = one(headers, "range").map_err(|_| Error::InvalidRange)?;
        let Some(raw) = raw else { return Ok(None) };
        if raw.len() > 1024 {
            return Err(Error::InvalidRange);
        }
        let (unit, value) = raw.trim().split_once('=').ok_or(Error::InvalidRange)?;
        if !unit.eq_ignore_ascii_case("bytes") {
            return Ok(None);
        }
        let (first, last) = value.split_once('-').ok_or(Error::InvalidRange)?;
        if first.is_empty() {
            let length = decimal(last)
                .filter(|length| *length > 0)
                .ok_or(Error::InvalidRange)?;
            return Ok(Some(Self::Suffix { length }));
        }
        let first = decimal(first).ok_or(Error::InvalidRange)?;
        if last.is_empty() {
            return Ok(Some(Self::Open { first }));
        }
        let last = decimal(last)
            .filter(|last| *last >= first)
            .ok_or(Error::InvalidRange)?;
        Ok(Some(Self::Bounded { first, last }))
    }

    pub(super) fn header(self) -> String {
        match self {
            Self::Bounded { first, last } => format!("bytes={first}-{last}"),
            Self::Open { first } => format!("bytes={first}-"),
            Self::Suffix { length } => format!("bytes=-{length}"),
        }
    }

    fn bounds(self, total: u64) -> Option<(u64, u64)> {
        if total == 0 {
            return None;
        }
        let (first, last) = match self {
            Self::Bounded { first, last } => (first, last.min(total - 1)),
            Self::Open { first } => (first, total - 1),
            Self::Suffix { length } => (total.saturating_sub(length), total - 1),
        };
        (first <= last && last < total).then_some((first, last))
    }
}

pub(super) fn verify(
    response: &reqwest::Response,
    selected: Option<Selection>,
) -> Result<Representation, Error> {
    let headers = response.headers();
    if headers.len() > 32
        || headers.contains_key("transfer-encoding")
        || one(headers, "content-encoding")?
            .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::InvalidResponse);
    }
    match response.status() {
        StatusCode::NOT_FOUND | StatusCode::GONE => return Err(Error::NotFound),
        StatusCode::TOO_MANY_REQUESTS => return Err(Error::Busy),
        status if status.is_server_error() => return Err(Error::Unavailable),
        _ => {}
    }
    let length = one(headers, "content-length")?
        .and_then(decimal)
        .filter(|length| *length > 0)
        .ok_or(Error::InvalidResponse)?;
    let mime = one(headers, "content-type")?.ok_or(Error::InvalidResponse)?;
    if mime.len() > 128 {
        return Err(Error::InvalidResponse);
    }
    let content_type = match mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "audio/mpeg" => "audio/mpeg",
        "audio/mp3" => "audio/mp3",
        "audio/ogg" => "audio/ogg",
        "application/ogg" => "application/ogg",
        "audio/flac" => "audio/flac",
        "audio/x-flac" => "audio/x-flac",
        "audio/wav" => "audio/wav",
        "audio/x-wav" => "audio/x-wav",
        "audio/wave" => "audio/wave",
        _ => return Err(Error::InvalidResponse),
    };
    let raw_range = one(headers, "content-range")?;
    let content_range = match selected {
        None if response.status() == StatusCode::OK && raw_range.is_none() => None,
        Some(selection) if response.status() == StatusCode::PARTIAL_CONTENT => {
            let value = raw_range.ok_or(Error::InvalidResponse)?;
            if value.len() > 128 {
                return Err(Error::InvalidResponse);
            }
            let (unit, range) = value.split_once(' ').ok_or(Error::InvalidResponse)?;
            let (bounds, total) = range.split_once('/').ok_or(Error::InvalidResponse)?;
            let (first, last) = bounds.split_once('-').ok_or(Error::InvalidResponse)?;
            let first = decimal(first).ok_or(Error::InvalidResponse)?;
            let last = decimal(last).ok_or(Error::InvalidResponse)?;
            let total = decimal(total).ok_or(Error::InvalidResponse)?;
            if !unit.eq_ignore_ascii_case("bytes")
                || selection.bounds(total) != Some((first, last))
                || last
                    .checked_sub(first)
                    .and_then(|difference| difference.checked_add(1))
                    != Some(length)
            {
                return Err(Error::InvalidResponse);
            }
            Some(format!("bytes {first}-{last}/{total}"))
        }
        _ => return Err(Error::InvalidResponse),
    };
    if selected.is_none()
        && !one(headers, "accept-ranges")?.is_some_and(|value| value.eq_ignore_ascii_case("bytes"))
    {
        return Err(Error::InvalidResponse);
    }
    Ok(Representation {
        content_type,
        length,
        content_range,
    })
}

pub(super) fn unsatisfied(response: &reqwest::Response) -> Result<u64, Error> {
    let value = one(response.headers(), "content-range")?.ok_or(Error::InvalidResponse)?;
    value
        .strip_prefix("bytes */")
        .and_then(decimal)
        .ok_or(Error::InvalidResponse)
}

fn one<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<Option<&'a str>, Error> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|value| value.to_str().map_err(|_| Error::InvalidResponse))
        .transpose()?;
    if values.next().is_some() {
        return Err(Error::InvalidResponse);
    }
    Ok(value)
}

fn decimal(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}
