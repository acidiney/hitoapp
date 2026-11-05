use super::Plugins;
use crate::{Actor, Error, Result};
use hito_plugin_sdk::{Authorization, Manifest};
use sha2::{Digest, Sha256};
use sqlx::Row;
use url::{Host, Url};

const MAX_URL_BYTES: usize = 4096;

pub(super) struct LiveAttempt<'a> {
    pub endpoint: &'a str,
    pub redirect_uri: &'a str,
    pub state_hash: &'a [u8],
}

pub(super) fn launch_url(candidate: &str, attempt: &LiveAttempt<'_>) -> Option<Url> {
    if candidate.len() > MAX_URL_BYTES
        || candidate
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
    {
        return None;
    }
    let url = Url::parse(candidate).ok()?;
    let endpoint = Url::parse(attempt.endpoint).ok()?;
    let secure = url.scheme() == "https"
        || (url.scheme() == "http"
            && match url.host() {
                Some(Host::Ipv4(address)) => address.is_loopback(),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                _ => false,
            });
    if !secure
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.scheme() != endpoint.scheme()
        || url.host() != endpoint.host()
        || url.port_or_known_default() != endpoint.port_or_known_default()
        || url.path() != endpoint.path()
    {
        return None;
    }
    let (mut state, mut redirect) = (None, None);
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "redirect_uri" => &mut redirect,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return None;
        }
    }
    let state_matches = state
        .is_some_and(|state| Sha256::digest(state.as_bytes()).as_slice() == attempt.state_hash);
    (state_matches && redirect.as_deref() == Some(attempt.redirect_uri)).then_some(url)
}

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), installation_id = id))]
    pub async fn authorization_launch(
        &self,
        actor: &Actor,
        id: &str,
        attempt_id: &str,
        candidate: &str,
    ) -> Result<Url> {
        actor.check_session(&self.store).await?;
        let row = sqlx::query("SELECT a.redirect_uri,a.state_hash,p.manifest FROM plugin_authorization_attempts a
            JOIN sessions session ON session.token_hash = a.session_hash AND session.user_id = a.user_id AND session.expires_at > unixepoch()
            JOIN plugin_installations p ON p.id = a.installation_id AND p.user_id = a.user_id AND p.service_id = a.service_id AND p.active = 1 AND p.revision = a.installation_revision
            JOIN plugin_services s ON s.id = a.service_id AND s.active = 1 AND s.revision = a.service_revision
            WHERE a.id = ? AND a.installation_id = ? AND a.user_id = ? AND a.session_hash = ? AND a.expires_at > unixepoch() AND a.status = 'pending'")
            .bind(attempt_id).bind(id).bind(actor.id()).bind(actor.session_binding())
            .fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let redirect_uri: String = row.try_get("redirect_uri")?;
        let state_hash: Vec<u8> = row.try_get("state_hash")?;
        let manifest: Manifest = serde_json::from_str(row.try_get("manifest")?)
            .map_err(|_| Error::Internal("Stored plugin manifest is invalid".into()))?;
        let Some(Authorization::Oauth2Pkce {
            authorization_endpoint,
            ..
        }) = manifest.authorization
        else {
            return Err(Error::NotFound);
        };
        let url = launch_url(
            candidate,
            &LiveAttempt {
                endpoint: &authorization_endpoint,
                redirect_uri: &redirect_uri,
                state_hash: &state_hash,
            },
        )
        .ok_or_else(|| {
            Error::Invalid("This is not the sign-in page for this connection attempt".into())
        })?;
        tracing::info!(
            outcome = "approved",
            "Provider sign-in page approved for the host browser"
        );
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "https://accounts.example/authorize";
    const REDIRECT: &str = "https://hito.test/api/v1/plugin-authorization/callback";

    fn hash(state: &str) -> Vec<u8> {
        Sha256::digest(state.as_bytes()).to_vec()
    }

    fn url(endpoint: &str, redirect: &str, state: &str) -> String {
        let mut url = Url::parse(endpoint).unwrap();
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", "client")
            .append_pair("redirect_uri", redirect)
            .append_pair("scope", "playlist-read-private")
            .append_pair("state", state)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", "challenge");
        url.into()
    }

    fn check(endpoint: &str, redirect: &str, candidate: &str) -> Option<Url> {
        launch_url(
            candidate,
            &LiveAttempt {
                endpoint,
                redirect_uri: redirect,
                state_hash: &hash("live-state"),
            },
        )
    }

    #[test]
    fn the_exact_server_url_is_accepted_for_https_and_literal_loopback() {
        let good = url(ENDPOINT, REDIRECT, "live-state");
        assert_eq!(check(ENDPOINT, REDIRECT, &good).unwrap().as_str(), good);
        for loopback in [
            "http://127.0.0.1:4100/authorize",
            "http://[::1]:4100/authorize",
        ] {
            let redirect = "http://127.0.0.1:9/api/v1/plugin-authorization/callback";
            let good = url(loopback, redirect, "live-state");
            assert_eq!(check(loopback, redirect, &good).unwrap().as_str(), good);
        }
    }

    #[test]
    fn every_other_url_is_refused() {
        let live = url(ENDPOINT, REDIRECT, "live-state");
        let other_state = url(ENDPOINT, REDIRECT, "other-state");
        let other_redirect = url(ENDPOINT, "https://elsewhere.test/cb", "live-state");
        let cases = [
            ("not a url", "empty relative text"),
            ("", "empty"),
            (
                &live.replace("https://accounts.example", "https://evil.example"),
                "other host",
            ),
            (
                &live.replace("accounts.example", "accounts.example:8443"),
                "other port",
            ),
            (&live.replace("https://", "http://"), "downgraded scheme"),
            (
                &live.replace("/authorize", "/authorize/other"),
                "other path",
            ),
            (&live.replace("https://", "https://user:pass@"), "userinfo"),
            (&format!("{live}#fragment"), "fragment"),
            (&other_state, "state of another attempt"),
            (&other_redirect, "redirect of another attempt"),
            ("file:///etc/passwd", "file scheme"),
            ("javascript:alert(1)", "script scheme"),
            ("ms-settings:privacy", "custom scheme"),
            (
                &live.replace("state=live-state", "state=live-state&state=live-state"),
                "duplicate state",
            ),
            (
                &format!("{live}&{}", "x".repeat(MAX_URL_BYTES)),
                "oversized",
            ),
            (&format!("{live}\n"), "control character"),
        ];
        for (candidate, reason) in cases {
            assert!(check(ENDPOINT, REDIRECT, candidate).is_none(), "{reason}");
        }
    }

    #[test]
    fn plain_http_is_refused_unless_the_host_is_a_loopback_ip() {
        let redirect = "http://127.0.0.1:9/api/v1/plugin-authorization/callback";
        for endpoint in [
            "http://accounts.example/authorize",
            "http://localhost:4100/authorize",
            "http://192.168.1.5:4100/authorize",
            "http://0.0.0.0:4100/authorize",
        ] {
            let candidate = url(endpoint, redirect, "live-state");
            assert!(
                check(endpoint, redirect, &candidate).is_none(),
                "{endpoint}"
            );
        }
    }
}
