use super::{
    EpisodeIdentity, FeedEpisode, FeedSnapshot, Result, invalid,
    xml::{Namespace, Node},
};
use std::collections::HashSet;
use url::Url;

pub(super) fn read(root: &Node, base: &Url) -> Result<FeedSnapshot> {
    validate_url(base)?;
    if !root.named(Namespace::Rss, "rss") || root.attribute("version") != Some("2.0") {
        return Err(invalid());
    }
    let channel = child(root, Namespace::Rss, "channel")?.ok_or_else(invalid)?;
    let mut snapshot = FeedSnapshot {
        title: required(channel, Namespace::Rss, "title", 2048)?,
        author: optional_field(channel, Namespace::Itunes, "author", 2048)?.or(optional_field(
            channel,
            Namespace::Rss,
            "managingEditor",
            2048,
        )?),
        description: optional_field(channel, Namespace::Rss, "description", 16_384)?,
        episodes: Vec::new(),
    };
    let mut identities = HashSet::new();
    for item in channel
        .children
        .iter()
        .filter(|node| node.named(Namespace::Rss, "item"))
    {
        let Some(episode) = episode(item, base)? else {
            continue;
        };
        if snapshot.episodes.len() >= 5000 || !identities.insert(identity_key(&episode.identity)) {
            return Err(invalid());
        }
        snapshot.episodes.push(episode);
    }
    Ok(snapshot)
}

fn episode(item: &Node, base: &Url) -> Result<Option<FeedEpisode>> {
    let mut selected = None;
    for enclosure in item
        .children
        .iter()
        .filter(|node| node.named(Namespace::Rss, "enclosure"))
    {
        let mime = enclosure
            .attribute("type")
            .map(|value| bounded(value, 256).map(str::to_owned))
            .transpose()?;
        if mime.as_deref().is_some_and(|mime| {
            let essence = mime
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            !(essence.starts_with("audio/") && essence.len() > 6 || essence == "application/ogg")
        }) {
            continue;
        }
        if selected.is_some() {
            return Err(invalid());
        }
        let raw = enclosure.attribute("url").ok_or_else(invalid)?;
        let url = enclosure_url(raw, base)?;
        let length = enclosure.attribute("length").map(unsigned).transpose()?;
        selected = Some((url, mime, length));
    }
    let Some((enclosure_url, declared_mime, length)) = selected else {
        return Ok(None);
    };
    let identity = match child(item, Namespace::Rss, "guid")? {
        Some(guid) => {
            if !guid.children.is_empty() {
                return Err(invalid());
            }
            bounded(&guid.text, 2048)?;
            EpisodeIdentity::Guid(guid.text.clone())
        }
        None => EpisodeIdentity::Enclosure(enclosure_url.clone()),
    };
    Ok(Some(FeedEpisode {
        identity,
        title: required(item, Namespace::Rss, "title", 2048)?,
        author: optional_field(item, Namespace::Itunes, "author", 2048)?.or(optional_field(
            item,
            Namespace::Rss,
            "author",
            2048,
        )?),
        description: optional_field(item, Namespace::Rss, "description", 16_384)?,
        enclosure_url,
        declared_mime,
        length,
        duration_ms: field(item, Namespace::Itunes, "duration", 64)?
            .map(|value| duration(&value))
            .transpose()?,
        published_at: field(item, Namespace::Rss, "pubDate", 256)?
            .map(|value| {
                chrono::DateTime::parse_from_rfc2822(&value)
                    .map(|date| date.timestamp())
                    .map_err(|_| invalid())
            })
            .transpose()?,
    }))
}

fn identity_key(identity: &EpisodeIdentity) -> (u8, String) {
    match identity {
        EpisodeIdentity::Guid(value) => (0, value.clone()),
        EpisodeIdentity::Enclosure(value) => (1, value.clone()),
    }
}

fn child<'a>(node: &'a Node, namespace: Namespace, name: &str) -> Result<Option<&'a Node>> {
    let mut matches = node
        .children
        .iter()
        .filter(|node| node.named(namespace, name));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(invalid());
    }
    Ok(first)
}

fn field(node: &Node, namespace: Namespace, name: &str, limit: usize) -> Result<Option<String>> {
    child(node, namespace, name)?
        .map(|value| {
            if !value.children.is_empty() {
                return Err(invalid());
            }
            bounded(value.text.trim(), limit).map(str::to_owned)
        })
        .transpose()
}

fn required(node: &Node, namespace: Namespace, name: &str, limit: usize) -> Result<String> {
    field(node, namespace, name, limit)?.ok_or_else(invalid)
}

fn optional_field(
    node: &Node,
    namespace: Namespace,
    name: &str,
    limit: usize,
) -> Result<Option<String>> {
    match child(node, namespace, name)? {
        Some(value) if value.children.is_empty() && value.text.trim().is_empty() => Ok(None),
        _ => field(node, namespace, name, limit),
    }
}

fn bounded(value: &str, limit: usize) -> Result<&str> {
    if value.trim().is_empty()
        || value.len() > limit
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(invalid());
    }
    Ok(value)
}

fn enclosure_url(raw: &str, base: &Url) -> Result<String> {
    if raw.is_empty() || raw.len() > 2048 || raw != raw.trim() || raw.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let authority = raw
        .split_once("://")
        .map(|(_, remainder)| remainder)
        .or_else(|| raw.strip_prefix("//"));
    if authority.is_some_and(|value| {
        value
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|value| value.contains('@'))
    }) {
        return Err(invalid());
    }
    match Url::parse(raw) {
        Ok(url) => {
            validate_url(&url)?;
            Ok(raw.to_owned())
        }
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            let url = base.join(raw).map_err(|_| invalid())?;
            validate_url(&url)?;
            Ok(url.into())
        }
        Err(_) => Err(invalid()),
    }
}

fn validate_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str().len() > 2048
    {
        return Err(invalid());
    }
    Ok(())
}

fn unsigned(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}

fn duration(value: &str) -> Result<u64> {
    let parts = value.split(':').map(unsigned).collect::<Result<Vec<_>>>()?;
    let seconds = match parts.as_slice() {
        [seconds] => *seconds,
        [minutes, seconds] if *seconds < 60 => minutes
            .checked_mul(60)
            .and_then(|value| value.checked_add(*seconds))
            .ok_or_else(invalid)?,
        [hours, minutes, seconds] if *minutes < 60 && *seconds < 60 => hours
            .checked_mul(3600)
            .and_then(|value| {
                minutes
                    .checked_mul(60)
                    .and_then(|minutes| value.checked_add(minutes))
            })
            .and_then(|value| value.checked_add(*seconds))
            .ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    seconds.checked_mul(1000).ok_or_else(invalid)
}
