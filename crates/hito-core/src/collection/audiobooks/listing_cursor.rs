use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_CURSOR_BYTES: usize = 4096;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Direction {
    Next,
    Previous,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cursor {
    version: u8,
    pub owner: String,
    pub name: String,
    pub playlist: Uuid,
    pub direction: Direction,
}

impl Cursor {
    pub(super) fn decode(value: &str) -> Result<Self> {
        if value.is_empty() || value.len() > MAX_CURSOR_BYTES {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| invalid())?;
        let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if cursor.version != 1
            || cursor.owner.is_empty()
            || cursor.owner.len() > 128
            || cursor.name.is_empty()
            || cursor.name.len() > 256
        {
            return Err(invalid());
        }
        Ok(cursor)
    }

    pub(super) fn encode(
        owner: &str,
        name: &str,
        playlist: &str,
        direction: Direction,
    ) -> Result<String> {
        let cursor = Self {
            version: 1,
            owner: owner.to_owned(),
            name: name.to_owned(),
            playlist: Uuid::parse_str(playlist)
                .map_err(|_| Error::Internal("An audiobook identity is invalid".into()))?,
            direction,
        };
        let bytes = serde_json::to_vec(&cursor)
            .map_err(|_| Error::Internal("An audiobook cursor could not be encoded".into()))?;
        let encoded = URL_SAFE_NO_PAD.encode(bytes);
        if encoded.len() > MAX_CURSOR_BYTES {
            return Err(Error::Internal(
                "An audiobook cursor exceeds its size limit".into(),
            ));
        }
        Ok(encoded)
    }
}

fn invalid() -> Error {
    Error::Invalid("This audiobook page cursor is invalid".into())
}
