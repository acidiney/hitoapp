use crate::recordings::valid_id;
use hito_plugin_sdk::MAX_SOURCE_LINK_REFERENCE_BYTES;
use url::Url;

const WATCH_HOSTS: [&str; 4] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
];
const SHORTS_HOSTS: [&str; 3] = ["youtube.com", "www.youtube.com", "m.youtube.com"];

pub(crate) fn video_id(reference: &str) -> Option<String> {
    if reference.len() > MAX_SOURCE_LINK_REFERENCE_BYTES || reference.chars().any(char::is_control)
    {
        return None;
    }
    let reference = reference.trim();
    if valid_id(reference) {
        return Some(reference.to_owned());
    }
    let address = if reference.contains("://") {
        Url::parse(reference)
    } else {
        Url::parse(&format!("https://{reference}"))
    }
    .ok()?;
    if !matches!(address.scheme(), "https" | "http")
        || !address.username().is_empty()
        || address.password().is_some()
        || address.port().is_some()
    {
        return None;
    }
    let host = address.host_str()?;
    let segments: Vec<&str> = address
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect();
    let id = match segments.as_slice() {
        ["watch"] if WATCH_HOSTS.contains(&host) => {
            let mut values = address
                .query_pairs()
                .filter(|(name, _)| name == "v")
                .map(|(_, value)| value.into_owned());
            let id = values.next()?;
            if values.next().is_some() {
                return None;
            }
            id
        }
        ["shorts", id] if SHORTS_HOSTS.contains(&host) => (*id).to_owned(),
        [id] if host == "youtu.be" => (*id).to_owned(),
        _ => return None,
    };
    valid_id(&id).then_some(id)
}
