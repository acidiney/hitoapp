use crate::error::Error;
use hito_plugin_sdk::{AnchoredLyricLine, MAX_LYRICS_RESPONSE_BYTES};
use std::collections::BTreeSet;

const MAX_TIME: u64 = 604_800_000;

pub(crate) fn anchors(raw: &str, duration: Option<u64>) -> Result<Vec<AnchoredLyricLine>, Error> {
    if raw.len() > MAX_LYRICS_RESPONSE_BYTES {
        return Err(Error::InvalidResponse);
    }
    let mut lines = Vec::new();
    let mut text_bytes = 0usize;
    for (index, raw_line) in raw.lines().enumerate() {
        if index >= 2000 || raw_line.len() > 4096 || raw_line.chars().any(char::is_control) {
            return Err(Error::InvalidResponse);
        }
        let line = raw_line.trim_start_matches(' ');
        if line.trim().is_empty() {
            continue;
        }
        if let Some(tag) = line
            .trim_end_matches(' ')
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
            && let Some((key, value)) = tag.split_once(':')
        {
            if key.eq_ignore_ascii_case("offset") {
                let value = value
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| Error::InvalidResponse)?;
                if value != 0 {
                    return Err(Error::InvalidResponse);
                }
                continue;
            }
            if ["ar", "ti", "al", "by", "re", "ve", "length"]
                .iter()
                .any(|known| key.eq_ignore_ascii_case(known))
            {
                continue;
            }
        }
        let (times, text) = anchored_line(line)?;
        for start_ms in times {
            text_bytes = text_bytes
                .checked_add(text.len())
                .ok_or(Error::InvalidResponse)?;
            if lines.len() == 2000 || text_bytes > MAX_LYRICS_RESPONSE_BYTES {
                return Err(Error::InvalidResponse);
            }
            lines.push(AnchoredLyricLine {
                start_ms,
                text: text.to_owned(),
            });
        }
    }
    if !lines.iter().any(|line| !line.text.is_empty()) {
        return Err(Error::InvalidResponse);
    }
    for line in &lines {
        if duration.is_some_and(|duration| line.start_ms > duration) {
            return Err(Error::InvalidResponse);
        }
    }
    lines.sort_by_key(|line| line.start_ms);
    let mut seen = BTreeSet::new();
    lines.retain(|line| seen.insert((line.start_ms, line.text.clone())));
    Ok(lines)
}

fn anchored_line(mut line: &str) -> Result<(Vec<u64>, &str), Error> {
    let mut times = Vec::new();
    while let Some(rest) = line.strip_prefix('[') {
        let Some((tag, remaining)) = rest.split_once(']') else {
            return Err(Error::InvalidResponse);
        };
        if !tag.as_bytes().first().is_some_and(u8::is_ascii_digit) && !times.is_empty() {
            break;
        }
        times.push(timestamp(tag).ok_or(Error::InvalidResponse)?);
        if times.len() > 2000 {
            return Err(Error::InvalidResponse);
        }
        line = remaining;
    }
    if times.is_empty() || line.len() > 2048 {
        return Err(Error::InvalidResponse);
    }
    Ok((times, line.trim()))
}

fn timestamp(value: &str) -> Option<u64> {
    let (minutes, seconds) = value.split_once(':')?;
    if minutes.is_empty()
        || minutes.len() > 5
        || !minutes.bytes().all(|value| value.is_ascii_digit())
    {
        return None;
    }
    let (seconds, fraction) = seconds
        .split_once('.')
        .map_or((seconds, None), |(seconds, fraction)| {
            (seconds, Some(fraction))
        });
    if seconds.len() != 2 || !seconds.bytes().all(|value| value.is_ascii_digit()) {
        return None;
    }
    let seconds = seconds.parse::<u64>().ok().filter(|value| *value < 60)?;
    let fraction = match fraction {
        None => 0,
        Some(value)
            if (1..=3).contains(&value.len())
                && value.bytes().all(|value| value.is_ascii_digit()) =>
        {
            value.parse::<u64>().ok()? * 10u64.pow(3 - value.len() as u32)
        }
        _ => return None,
    };
    minutes
        .parse::<u64>()
        .ok()?
        .checked_mul(60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(fraction)
        .filter(|time| *time <= MAX_TIME)
}
