use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use url::{Host, Url};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Authorization {
    Oauth2Pkce {
        client_id: String,
        authorization_endpoint: String,
        scopes: BTreeSet<String>,
    },
}

impl Authorization {
    pub fn validate(&self) -> Result<(), &'static str> {
        let Self::Oauth2Pkce {
            client_id,
            authorization_endpoint,
            scopes,
        } = self;
        if client_id.is_empty()
            || client_id.len() > 256
            || !client_id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return Err("OAuth client IDs must contain 1 to 256 visible ASCII bytes");
        }
        validate_endpoint(authorization_endpoint)?;
        if scopes.len() > 32
            || scopes.iter().any(|scope| {
                scope.is_empty()
                    || scope.len() > 128
                    || !scope
                        .bytes()
                        .all(|byte| matches!(byte, b'!' | b'#'..=b'[' | b']'..=b'~'))
            })
        {
            return Err("Declare at most 32 OAuth scope tokens of 1 to 128 bytes");
        }
        Ok(())
    }
}

fn validate_endpoint(endpoint: &str) -> Result<(), &'static str> {
    const INVALID: &str = "OAuth authorization endpoints must use HTTPS or a loopback IP over HTTP without credentials, query, or fragment";
    if endpoint.len() > 2048
        || endpoint
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
    {
        return Err(INVALID);
    }
    let endpoint = Url::parse(endpoint).map_err(|_| INVALID)?;
    let allowed_scheme = endpoint.scheme() == "https"
        || (endpoint.scheme() == "http"
            && match endpoint.host() {
                Some(Host::Ipv4(address)) => address.is_loopback(),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                _ => false,
            });
    if !allowed_scheme
        || endpoint.host().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(INVALID);
    }
    Ok(())
}
