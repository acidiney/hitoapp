use crate::{error::Error, recording::Record};
use hito_plugin_sdk::{LyricAttribution, LyricContent, LyricsDocument};

pub(crate) fn document(record: &Record) -> Result<Option<LyricsDocument>, Error> {
    if let Some(raw) = &record.plain_lyrics {
        validate_raw(raw, 2048)?;
    }
    if let Some(raw) = &record.synced_lyrics {
        validate_raw(raw, 4096)?;
    }
    let has_timing = record
        .synced_lyrics
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty());
    if record.instrumental {
        if has_timing
            || record
                .plain_lyrics
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
        {
            return Err(Error::InvalidResponse);
        }
        return Ok(None);
    }
    let duration = record.recording()?.duration_ms;
    let content = match record
        .synced_lyrics
        .as_deref()
        .and_then(|raw| crate::lrc::anchors(raw, duration).ok())
    {
        Some(lines) => LyricContent::Anchored { lines },
        None => {
            let Some(raw) = record.plain_lyrics.as_deref() else {
                return if has_timing {
                    Err(Error::InvalidResponse)
                } else {
                    Ok(None)
                };
            };
            let lines = plain(raw);
            if !lines.iter().any(|line| !line.trim().is_empty()) {
                return if has_timing {
                    Err(Error::InvalidResponse)
                } else {
                    Ok(None)
                };
            }
            LyricContent::Plain { lines }
        }
    };
    Ok(Some(LyricsDocument {
        id: format!("lrclib:{}", record.id),
        rendition_id: format!("lrclib-record:{}", record.id),
        revision: None,
        rendition_label: None,
        language: None,
        duration_ms: duration,
        attribution: Some(LyricAttribution {
            provider_label: "LRCLIB".into(),
            content_url: format!("https://lrclib.net/api/get/{}", record.id),
            creators: Vec::new(),
        }),
        content,
    }))
}

fn validate_raw(raw: &str, line_bytes: usize) -> Result<(), Error> {
    if raw.len() > hito_plugin_sdk::MAX_LYRICS_RESPONSE_BYTES {
        return Err(Error::InvalidResponse);
    }
    let mut lines = raw.split('\n').peekable();
    let mut count = 0;
    while let Some(raw_line) = lines.next() {
        if raw_line.len() > line_bytes || count == 2000 {
            return Err(Error::InvalidResponse);
        }
        let line = if lines.peek().is_some() {
            raw_line.strip_suffix('\r').unwrap_or(raw_line)
        } else {
            raw_line
        };
        if line.chars().any(char::is_control) {
            return Err(Error::InvalidResponse);
        }
        count += 1;
    }
    Ok(())
}

fn plain(raw: &str) -> Vec<String> {
    raw.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}
