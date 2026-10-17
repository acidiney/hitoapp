use crate::{Error, recordings};

const MAX_AUDIO_BYTES: u64 = 128 * 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) struct Identity {
    owner: String,
    format: String,
    length: u64,
    modified: u64,
}

impl Identity {
    pub(super) fn new(
        owner: &str,
        format: &str,
        length: u64,
        modified: u64,
    ) -> Result<Self, Error> {
        if !recordings::valid_id(owner)
            || format.is_empty()
            || format.len() > 16
            || !format
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || !(1..=MAX_AUDIO_BYTES).contains(&length)
        {
            return Err(Error::InvalidResponse);
        }
        Ok(Self {
            owner: owner.to_owned(),
            format: format.to_owned(),
            length,
            modified,
        })
    }

    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        if value.len() > 256 {
            return Err(Error::NotFound);
        }
        let mut parts = value.split(':');
        if parts.next() != Some("youtube") || parts.next() != Some("v1") {
            return Err(Error::NotFound);
        }
        let owner = parts.next().ok_or(Error::NotFound)?;
        let encoded = parts.next().ok_or(Error::NotFound)?;
        if parts.next().is_some()
            || !recordings::valid_id(owner)
            || encoded.is_empty()
            || encoded.len() > 94
            || !encoded.len().is_multiple_of(2)
        {
            return Err(Error::NotFound);
        }
        let bytes: Vec<u8> = encoded
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| Ok(nibble(pair[0])? * 16 + nibble(pair[1])?))
            .collect::<Result<_, Error>>()?;
        let witness = std::str::from_utf8(&bytes).map_err(|_| Error::NotFound)?;
        let mut fields = witness.split(':');
        let format = fields.next().ok_or(Error::NotFound)?;
        let length = decimal(fields.next().ok_or(Error::NotFound)?)?;
        let modified = decimal(fields.next().ok_or(Error::NotFound)?)?;
        if fields.next().is_some() {
            return Err(Error::NotFound);
        }
        let identity = Self::new(owner, format, length, modified).map_err(|_| Error::NotFound)?;
        if identity.encode() != value {
            return Err(Error::NotFound);
        }
        Ok(identity)
    }

    pub(super) fn owner(&self) -> &str {
        &self.owner
    }

    pub(super) fn encode(&self) -> String {
        let witness = format!("{}:{}:{}", self.format, self.length, self.modified);
        let mut encoded = format!("youtube:v1:{}:", self.owner);
        for byte in witness.bytes() {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
        encoded
    }
}

fn nibble(byte: u8) -> Result<u8, Error> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Error::NotFound),
    }
}

fn decimal(value: &str) -> Result<u64, Error> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Error::NotFound);
    }
    value.parse().map_err(|_| Error::NotFound)
}
