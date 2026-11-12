mod endpoint;
mod logging;
mod request;
mod response;

use crate::{
    AuthSession, BeginCatalogSnapshot, CatalogSnapshot, CatalogSnapshotPage, Credentials,
    SyncBatch, SyncPage, SyncReceipt, User,
};
pub(super) use endpoint::Endpoint;
use reqwest::{Client, Method, RequestBuilder};
use std::future::Future;
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

pub(super) const MAX_REQUEST_BYTES: usize = 262_144;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RemoteError {
    Unauthorized,
    Locked,
    Forbidden,
    NotFound,
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
        let request = request::json(
            self.request("api/v1/login", Method::POST, None)?,
            &credentials,
        )?;
        let session: AuthSession = self
            .exchange(request, "login", |reply| response::decode(reply, 65_536))
            .await?;
        validate_token(&session.token)?;
        validate_account(&session.user.id)?;
        Ok(session)
    }

    pub(super) async fn me(&self, token: &str, account: &str) -> RemoteResult<User> {
        let request = self.authenticated("api/v1/me", Method::GET, token, account, None)?;
        let user: User = self
            .exchange(request, "me", |reply| response::decode(reply, 65_536))
            .await?;
        if user.id != account {
            return Err(RemoteError::InvalidReply);
        }
        Ok(user)
    }

    pub(super) async fn begin_catalog_snapshot(
        &self,
        token: &str,
        account: &str,
        snapshot: &BeginCatalogSnapshot,
    ) -> RemoteResult<CatalogSnapshot> {
        let request = request::json(
            self.authenticated(
                "api/v1/catalog/snapshots",
                Method::POST,
                token,
                account,
                None,
            )?,
            snapshot,
        )?;
        self.exchange(request, "begin_catalog_snapshot", |reply| {
            response::decode(reply, 65_536)
        })
        .await
    }

    pub(super) async fn catalog_snapshot_page(
        &self,
        token: &str,
        account: &str,
        id: Uuid,
        ordinal: u64,
    ) -> RemoteResult<(CatalogSnapshotPage, usize)> {
        let request = self.authenticated(
            endpoint::Route::CatalogPage { id, ordinal },
            Method::GET,
            token,
            account,
            None,
        )?;
        self.exchange(request, "catalog_snapshot_page", |reply| {
            response::decode_counted(reply, 262_144)
        })
        .await
    }

    pub(super) async fn release_catalog_snapshot(
        &self,
        token: &str,
        account: &str,
        id: Uuid,
    ) -> RemoteResult<()> {
        let request = self.authenticated(
            endpoint::Route::CatalogLease(id),
            Method::DELETE,
            token,
            account,
            None,
        )?;
        self.exchange(request, "release_catalog_snapshot", response::empty)
            .await
    }

    pub(super) async fn push(
        &self,
        token: &str,
        account: &str,
        batch: &SyncBatch,
    ) -> RemoteResult<SyncReceipt> {
        let request = request::json(
            self.authenticated("api/v1/sync", Method::POST, token, account, None)?,
            batch,
        )?;
        self.exchange(request, "push", |reply| response::decode(reply, 65_536))
            .await
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
        let request =
            self.authenticated("api/v1/sync", Method::GET, token, account, Some(after))?;
        self.exchange(request, "pull", |reply| {
            response::decode(reply, 16 * 1024 * 1024)
        })
        .await
    }

    fn request(
        &self,
        path: impl Into<endpoint::Route>,
        method: Method,
        cursor: Option<i64>,
    ) -> RemoteResult<RequestBuilder> {
        let mut endpoint = self.endpoint.route(path)?;
        if let Some(cursor) = cursor {
            endpoint
                .query_pairs_mut()
                .append_pair("cursor", &cursor.to_string());
        }
        let request = self.client.request(method, endpoint);
        Ok(request
            .header("x-hito-request", "1")
            .header("accept", "application/json")
            .header("accept-encoding", "identity"))
    }

    fn authenticated(
        &self,
        path: impl Into<endpoint::Route>,
        method: Method,
        token: &str,
        account: &str,
        cursor: Option<i64>,
    ) -> RemoteResult<RequestBuilder> {
        validate_token(token)?;
        validate_account(account)?;
        Ok(self
            .request(path, method, cursor)?
            .bearer_auth(token)
            .header("x-hito-account", account))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %Uuid::new_v4(), operation = operation))]
    async fn exchange<T, F: Future<Output = RemoteResult<T>>>(
        &self,
        request: RequestBuilder,
        operation: &'static str,
        decode: impl FnOnce(reqwest::Response) -> F,
    ) -> RemoteResult<T> {
        let mut log = logging::Exchange::started();
        let result = async {
            let response = tokio::time::timeout(self.headers_timeout, request.send())
                .await
                .map_err(|_| RemoteError::Unavailable)?
                .map_err(|_| RemoteError::Unavailable)?;
            tokio::time::timeout(self.body_timeout, decode(response))
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
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::InvalidReply => "invalid_reply",
            Self::InvalidRequest => "invalid_request",
        }
    }
}
