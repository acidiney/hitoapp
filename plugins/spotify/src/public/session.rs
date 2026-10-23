use crate::error::Error;
use serde::{Deserialize, Deserializer};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Bootstrap {
    props: Props,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Props {
    page_props: PageProps,
}

#[derive(Deserialize)]
struct PageProps {
    state: State,
}

#[derive(Deserialize)]
struct State {
    settings: Settings,
}

#[derive(Deserialize)]
struct Settings {
    session: SessionData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionData {
    #[serde(deserialize_with = "deserialize_token")]
    access_token: Zeroizing<String>,
    access_token_expiration_timestamp_ms: u64,
}

pub(super) struct Session {
    pub token: Zeroizing<String>,
    expiration_ms: u64,
}

impl Session {
    pub(super) fn validate_expiration(&self) -> Result<(), Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_millis();
        if u128::from(self.expiration_ms) <= now {
            return Err(Error::Unavailable);
        }
        Ok(())
    }
}

pub(super) fn read(bytes: &[u8]) -> Result<Session, Error> {
    let html = std::str::from_utf8(bytes).map_err(|_| Error::InvalidResponse)?;
    let data = script_contents(html)?;
    let bootstrap: Bootstrap = serde_json::from_str(data).map_err(|_| Error::InvalidResponse)?;
    let data = bootstrap.props.page_props.state.settings.session;
    let session = Session {
        token: data.access_token,
        expiration_ms: data.access_token_expiration_timestamp_ms,
    };
    if session.token.is_empty()
        || session.token.len() > 4096
        || !session
            .token
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(Error::InvalidResponse);
    }
    session.validate_expiration()?;
    Ok(session)
}

fn deserialize_token<'de, D>(deserializer: D) -> Result<Zeroizing<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Zeroizing::new)
}

pub(super) fn script_contents(html: &str) -> Result<&str, Error> {
    let mut remaining = html;
    let mut found = None;
    while let Some(start) = remaining.find("<script") {
        remaining = &remaining[start + "<script".len()..];
        if !remaining
            .starts_with(|character: char| character.is_ascii_whitespace() || character == '>')
        {
            continue;
        }
        let end = remaining.find('>').ok_or(Error::InvalidResponse)?;
        let selected = next_data_id(&remaining[..end])?;
        remaining = &remaining[end + 1..];
        let end = remaining.find("</script>").ok_or(Error::InvalidResponse)?;
        if selected && found.replace(&remaining[..end]).is_some() {
            return Err(Error::InvalidResponse);
        }
        remaining = &remaining[end + "</script>".len()..];
    }
    found.ok_or(Error::InvalidResponse)
}

fn next_data_id(mut attributes: &str) -> Result<bool, Error> {
    let mut selected = false;
    loop {
        attributes = attributes.trim_start();
        if attributes.is_empty() {
            break;
        }
        let end = attributes
            .find(|character: char| character.is_ascii_whitespace() || character == '=')
            .unwrap_or(attributes.len());
        let name = &attributes[..end];
        attributes = attributes[end..].trim_start();
        if !attributes.starts_with('=') {
            continue;
        }
        attributes = attributes[1..].trim_start();
        let quote = attributes.chars().next().ok_or(Error::InvalidResponse)?;
        let value;
        if quote == '\'' || quote == '"' {
            attributes = &attributes[1..];
            let end = attributes.find(quote).ok_or(Error::InvalidResponse)?;
            value = &attributes[..end];
            attributes = &attributes[end + 1..];
        } else {
            let end = attributes
                .find(char::is_whitespace)
                .unwrap_or(attributes.len());
            value = &attributes[..end];
            attributes = &attributes[end..];
        }
        if name.eq_ignore_ascii_case("id") && value == "__NEXT_DATA__" {
            if selected {
                return Err(Error::InvalidResponse);
            }
            selected = true;
        }
    }
    Ok(selected)
}
