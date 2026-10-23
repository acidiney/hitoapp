use super::{Admission, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{ARTWORK_PATH, ArtworkRequest};

pub(crate) const MAX_ARTWORK_BYTES: usize = 2 * 1024 * 1024;

pub(crate) struct ProviderArtwork {
    pub(crate) content_type: &'static str,
    pub(crate) bytes: Vec<u8>,
}

impl ControlClient {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(in crate::plugins) async fn artwork(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &ArtworkRequest,
    ) -> Result<Option<ProviderArtwork>> {
        request.validate().map_err(|_| invalid())?;
        let _admission = match self.admit_artwork(account) {
            Ok(admission) => admission,
            Err(Error::Conflict(_)) => {
                tracing::debug!(
                    event = "artwork_admission_deferred",
                    "Background artwork transport is busy"
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let endpoint = crate::plugins::offers::endpoint(base_url)?
            .join(ARTWORK_PATH)
            .map_err(|_| Error::Invalid("Invalid plugin service address".into()))?;
        let mut outgoing = self
            .client
            .post(endpoint)
            .header("accept", "image/png, image/jpeg")
            .header("accept-encoding", "identity")
            .header("x-hito-plugin-protocol", "1")
            .json(request);
        if let Some(token) = token {
            crate::plugins::vault::validate(token)?;
            outgoing = outgoing.bearer_auth(token);
        }
        let mut response = outgoing
            .send()
            .await
            .map_err(|_| Error::Upstream("The artwork provider could not be reached"))?;
        let status = response.status();
        tracing::debug!(
            status = status.as_u16(),
            "Provider artwork headers received"
        );
        match status.as_u16() {
            200 => {}
            204 | 404 => return Err(Error::NotFound),
            429 | 500..=599 => return Err(Error::Upstream("The artwork provider is unavailable")),
            _ => return Err(invalid()),
        }
        let headers = response.headers();
        if headers.len() > 32
            || headers
                .iter()
                .map(|(name, value)| name.as_str().len() + value.len())
                .sum::<usize>()
                > 16 * 1024
        {
            return Err(invalid());
        }
        if one(headers, "content-encoding")?
            .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
            || one(headers, "content-range")?.is_some()
        {
            return Err(invalid());
        }
        let content_type = match one(headers, "content-type")?
            .and_then(|value| value.split(';').next())
            .map(str::trim)
        {
            Some(value) if value.eq_ignore_ascii_case("image/png") => "image/png",
            Some(value) if value.eq_ignore_ascii_case("image/jpeg") => "image/jpeg",
            _ => return Err(invalid()),
        };
        let length = one(headers, "content-length")?
            .map(|value| {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid());
                }
                value.parse::<usize>().map_err(|_| invalid())
            })
            .transpose()?;
        if length.is_some_and(|length| length == 0 || length > MAX_ARTWORK_BYTES)
            || one(headers, "transfer-encoding")?
                .is_some_and(|value| !value.eq_ignore_ascii_case("chunked") || length.is_some())
        {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Upstream("The artwork provider response was interrupted"))?
        {
            if chunk.len() > MAX_ARTWORK_BYTES.saturating_sub(bytes.len()) {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() || length.is_some_and(|length| length != bytes.len()) {
            return Err(invalid());
        }
        Ok(Some(ProviderArtwork {
            content_type,
            bytes,
        }))
    }

    fn admit_artwork(&self, account: &str) -> Result<Admission> {
        let account = self.artwork_accounts.enter(account)?;
        let global = self
            .artwork_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Background artwork requests are busy".into()))?;
        Ok(Admission {
            _account: account,
            _global: global,
        })
    }
}

fn one<'a>(headers: &'a reqwest::header::HeaderMap, name: &'static str) -> Result<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|value| value.to_str().map_err(|_| invalid()))
        .transpose()?;
    if values.next().is_some() {
        return Err(invalid());
    }
    Ok(value)
}

fn invalid() -> Error {
    Error::Invalid("The provider returned invalid artwork".into())
}
