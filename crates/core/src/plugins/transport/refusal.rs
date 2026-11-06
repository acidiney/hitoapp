use crate::{Error, identity::is_hidden};

const BODY_LIMIT: usize = 4096;
const MESSAGE_LIMIT: usize = 240;
const GENERIC: &str = "The plugin service rejected the request";

pub(super) async fn read(mut response: reqwest::Response, maximum: usize) -> Error {
    let limit = maximum.min(BODY_LIMIT);
    let json = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    let identity = response
        .headers()
        .get("content-encoding")
        .is_none_or(|value| value == "identity");
    if !json || !identity {
        return Error::Upstream(GENERIC);
    }
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Error::Upstream(GENERIC);
        }
        bytes.extend_from_slice(&chunk);
    }
    match message(&bytes) {
        Some(message) => Error::UpstreamNotice(message),
        None => Error::Upstream(GENERIC),
    }
}

fn message(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let text: String = value
        .get("error")?
        .as_str()?
        .chars()
        .filter(|character| !is_hidden(*character))
        .collect();
    let text = text.trim();
    let lowered = text.to_ascii_lowercase();
    if text.is_empty() || lowered.contains("http://") || lowered.contains("https://") {
        return None;
    }
    Some(
        text.chars()
            .take(MESSAGE_LIMIT)
            .collect::<String>()
            .trim_end()
            .to_string(),
    )
}
