use super::AudioValidators;
use crate::{AudioContent, AudioRequest, Error, OpenedAudio, Result};
use reqwest::{StatusCode, header::HeaderMap};

pub(crate) fn read(response: &reqwest::Response, request: AudioRequest) -> Result<OpenedAudio> {
    let headers = response.headers();
    if let Some(value) = one(headers, "transfer-encoding")?
        && (!value.trim().eq_ignore_ascii_case("chunked") || headers.contains_key("content-length"))
    {
        return invalid();
    }
    if one(headers, "content-encoding")?
        .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return invalid();
    }
    let raw_range = one(headers, "content-range")?;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        if request.head || request.range.is_none() {
            return invalid();
        }
        let length = raw_range
            .map(|value| {
                value
                    .split_once(' ')
                    .filter(|(unit, _)| unit.eq_ignore_ascii_case("bytes"))
                    .and_then(|(_, range)| range.strip_prefix("*/"))
                    .and_then(decimal)
                    .ok_or(Error::Upstream(
                        "The audio service returned an invalid range",
                    ))
            })
            .transpose()?;
        return Ok(OpenedAudio::Unsatisfiable { length });
    }
    let mut length = one(headers, "content-length")?
        .map(|value| {
            decimal(value).ok_or(Error::Upstream(
                "The audio service returned an invalid length",
            ))
        })
        .transpose()?;
    let (range, total_length) = match response.status() {
        StatusCode::OK if raw_range.is_none() => (None, length),
        StatusCode::PARTIAL_CONTENT if !request.head => {
            let (received, total) = raw_range.and_then(content_range).ok_or(Error::Upstream(
                "The audio service returned an invalid range",
            ))?;
            if !request
                .range
                .is_some_and(|requested| requested.accepts(&received, total))
            {
                return invalid();
            }
            let selected = received
                .end()
                .checked_sub(*received.start())
                .and_then(|length| length.checked_add(1))
                .ok_or(Error::Upstream(
                    "The audio service returned an invalid range length",
                ))?;
            if length.is_some_and(|length| length != selected) {
                return invalid();
            }
            length = Some(selected);
            (Some(received), total)
        }
        _ => return invalid(),
    };
    let content_type =
        one(headers, "content-type")?
            .and_then(audio_type)
            .ok_or(Error::Upstream(
                "The service did not return a supported audio type",
            ))?;
    let seekable = range.is_some()
        || one(headers, "accept-ranges")?.is_some_and(|value| value.eq_ignore_ascii_case("bytes"));
    Ok(OpenedAudio::Content(Box::new(AudioContent {
        content_type,
        length,
        total_length,
        range,
        seekable,
        validators: AudioValidators::read(headers),
        body: None,
    })))
}
fn invalid<T>() -> Result<T> {
    Err(Error::Upstream(
        "The audio service returned an invalid representation",
    ))
}

fn one<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|value| {
            value
                .to_str()
                .map_err(|_| Error::Upstream("The audio service returned an invalid header"))
        })
        .transpose()?;
    if values.next().is_some() {
        return invalid();
    }
    Ok(value)
}
fn decimal(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}
fn content_range(value: &str) -> Option<(std::ops::RangeInclusive<u64>, Option<u64>)> {
    if value.len() > 128 {
        return None;
    }
    let (unit, parts) = value.split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = parts.split_once('/')?;
    let (first, last) = range.split_once('-')?;
    let (first, last) = (decimal(first)?, decimal(last)?);
    let total = if total == "*" {
        None
    } else {
        Some(decimal(total)?)
    };
    if first > last || total.is_some_and(|total| total <= last) {
        return None;
    }
    Some((first..=last, total))
}
fn audio_type(value: &str) -> Option<&'static str> {
    if value.len() > 128 {
        return None;
    }
    match value
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("audio/wav"),
        "audio/mpeg" | "audio/mp3" => Some("audio/mpeg"),
        "audio/flac" | "audio/x-flac" => Some("audio/flac"),
        "audio/ogg" | "application/ogg" => Some("audio/ogg"),
        "audio/opus" => Some("audio/opus"),
        "audio/mp4" | "audio/x-m4a" => Some("audio/mp4"),
        "audio/aac" => Some("audio/aac"),
        "audio/aiff" | "audio/x-aiff" => Some("audio/aiff"),
        "audio/x-ms-wma" => Some("audio/x-ms-wma"),
        _ => None,
    }
}
