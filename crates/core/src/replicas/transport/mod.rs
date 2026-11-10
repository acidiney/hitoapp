mod endpoint;
mod logging;
mod request;
mod response;

use crate::{AuthSession, Credentials, SyncBatch, SyncPage, SyncReceipt, User};
pub(super) use endpoint::Endpoint;
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

pub(super) const MAX_REQUEST_BYTES: usize = 262_144;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RemoteError {
    Unauthorized,
    Locked,
    Forbidden,
    Conflict,
    RateLimited { retry_after: Option<Duration> },
    Unavailable,
    InvalidReply,
    InvalidRequest,
}

pub(super) type RemoteResult<T> = std::result::Result<T, RemoteError>;

pub(super) struct Remote {
    endpoint: Endpoint,
    client: Client,
    headers_timeout: Duration,
    body_timeout: Duration,
}

impl Remote {
    pub(super) fn new(endpoint: Endpoint) -> crate::Result<Self> {
        Self::with_deadlines(endpoint, Duration::from_secs(10), Duration::from_secs(15))
    }

    pub(super) fn with_deadlines(
        endpoint: Endpoint,
        headers_timeout: Duration,
        body_timeout: Duration,
    ) -> crate::Result<Self> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = Client::builder()
            .no_proxy()
            .referer(false)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .http1_max_headers(64)
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(5).min(headers_timeout))
            .timeout(headers_timeout + body_timeout)
            .user_agent(concat!("Hito/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                crate::Error::Internal("Replica transport could not be initialized".into())
            })?;
        Ok(Self {
            endpoint,
            client,
            headers_timeout,
            body_timeout,
        })
    }

    pub(super) async fn login(&self, credentials: Credentials) -> RemoteResult<AuthSession> {
        let request = request::json(self.request("api/v1/login", true, None)?, &credentials)?;
        let session: AuthSession = self.exchange(request, "login", 65_536).await?;
        validate_token(&session.token)?;
        validate_account(&session.user.id)?;
        Ok(session)
    }

    pub(super) async fn me(&self, token: &str, account: &str) -> RemoteResult<User> {
        let request = self.authenticated("api/v1/me", false, token, account, None)?;
        let user: User = self.exchange(request, "me", 65_536).await?;
        if user.id != account {
            return Err(RemoteError::InvalidReply);
        }
        Ok(user)
    }

    pub(super) async fn push(
        &self,
        token: &str,
        account: &str,
        batch: &SyncBatch,
    ) -> RemoteResult<SyncReceipt> {
        let request = request::json(
            self.authenticated("api/v1/sync", true, token, account, None)?,
            batch,
        )?;
        self.exchange(request, "push", 65_536).await
    }

    pub(super) async fn pull(
        &self,
        token: &str,
        account: &str,
        after: i64,
    ) -> RemoteResult<SyncPage> {
        if after < 0 {
            return Err(RemoteError::InvalidReply);
        }
        let request = self.authenticated("api/v1/sync", false, token, account, Some(after))?;
        self.exchange(request, "pull", 16 * 1024 * 1024).await
    }

    fn request(
        &self,
        path: &'static str,
        mutation: bool,
        cursor: Option<i64>,
    ) -> RemoteResult<RequestBuilder> {
        let mut endpoint = self.endpoint.route(path)?;
        if let Some(cursor) = cursor {
            endpoint
                .query_pairs_mut()
                .append_pair("cursor", &cursor.to_string());
        }
        let request = if mutation {
            self.client.post(endpoint)
        } else {
            self.client.get(endpoint)
        };
        Ok(request
            .header("x-hito-request", "1")
            .header("accept", "application/json")
            .header("accept-encoding", "identity"))
    }

    fn authenticated(
        &self,
        path: &'static str,
        mutation: bool,
        token: &str,
        account: &str,
        cursor: Option<i64>,
    ) -> RemoteResult<RequestBuilder> {
        validate_token(token)?;
        validate_account(account)?;
        Ok(self
            .request(path, mutation, cursor)?
            .bearer_auth(token)
            .header("x-hito-account", account))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %Uuid::new_v4(), operation = operation))]
    async fn exchange<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        operation: &'static str,
        limit: usize,
    ) -> RemoteResult<T> {
        let mut log = logging::Exchange::started();
        let result = async {
            let response = tokio::time::timeout(self.headers_timeout, request.send())
                .await
                .map_err(|_| RemoteError::Unavailable)?
                .map_err(|_| RemoteError::Unavailable)?;
            tokio::time::timeout(self.body_timeout, response::decode(response, limit))
                .await
                .map_err(|_| RemoteError::Unavailable)?
        }
        .with_subscriber(tracing::Dispatch::none())
        .await;
        let outcome = match &result {
            Ok(_) => "complete",
            Err(error) => error.kind(),
        };
        log.finished(outcome);
        result
    }
}

fn validate_token(token: &str) -> RemoteResult<()> {
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(RemoteError::InvalidReply);
    }
    Ok(())
}

fn validate_account(account: &str) -> RemoteResult<()> {
    if account.is_empty()
        || account.len() > 128
        || !account.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(RemoteError::InvalidReply);
    }
    Ok(())
}

impl RemoteError {
    fn kind(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Locked => "locked",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::InvalidReply => "invalid_reply",
            Self::InvalidRequest => "invalid_request",
        }
    }
}
