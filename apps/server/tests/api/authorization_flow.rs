use super::provider_fixture::ProviderServer;
use super::support::{admin, call, services};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::BTreeMap,
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;

pub(super) const CODE: &str = "owned-provider-code-sentinel";
pub(super) const ACCESS: &str = "owned-access-token-sentinel";
pub(super) const REFRESH: &str = "owned-refresh-token-sentinel";

#[derive(Clone, Copy)]
pub(super) enum Reply {
    Grant,
    Rejected,
    Redirect,
    Oversized,
    NotJson,
    NoAccessToken,
    GrantStringExpiry,
    GrantUpperIdentity,
    GrantAs(&'static str),
}

#[derive(Clone, Copy)]
pub(super) enum RefreshReply {
    Rotate,
    Keep,
    InvalidGrant,
    Unavailable,
    TooManyRequests,
    Malformed,
}

pub(super) struct TokenState {
    pub(super) requests: Mutex<Vec<BTreeMap<String, String>>>,
    pub(super) reply: Mutex<Reply>,
    pub(super) delay: Mutex<Duration>,
    pub(super) refresh: Mutex<RefreshReply>,
    pub(super) lifetime: Mutex<u64>,
    pub(super) refreshes: std::sync::atomic::AtomicUsize,
}

pub(super) struct TokenServer {
    pub(super) url: String,
    pub(super) state: Arc<TokenState>,
    pub(super) shutdown: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::task::JoinHandle<()>,
        )>,
    >,
}

impl TokenServer {
    pub(super) async fn start() -> Self {
        let state = Arc::new(TokenState {
            requests: Mutex::default(),
            reply: Mutex::new(Reply::Grant),
            delay: Mutex::new(Duration::ZERO),
            refresh: Mutex::new(RefreshReply::Rotate),
            lifetime: Mutex::new(3600),
            refreshes: std::sync::atomic::AtomicUsize::new(0),
        });
        let router = Router::new()
            .route("/token", axum::routing::post(token))
            .route("/elsewhere", axum::routing::post(|| async { "moved" }))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .unwrap()
        });
        Self {
            url,
            state,
            shutdown: Mutex::new(Some((stop, server))),
        }
    }

    pub(super) fn reply(&self, reply: Reply) {
        *self.state.reply.lock().unwrap() = reply;
    }

    pub(super) async fn stop(&self) {
        let running = self.shutdown.lock().unwrap().take();
        if let Some((stop, server)) = running {
            let _ = stop.send(());
            server.await.unwrap();
        }
    }

    pub(super) fn refresh_reply(&self, reply: RefreshReply) {
        *self.state.refresh.lock().unwrap() = reply;
    }

    pub(super) fn lifetime(&self, seconds: u64) {
        *self.state.lifetime.lock().unwrap() = seconds;
    }

    pub(super) fn refresh_requests(&self) -> Vec<BTreeMap<String, String>> {
        self.requests()
            .into_iter()
            .filter(|request| request["grant_type"] == "refresh_token")
            .collect()
    }

    pub(super) fn delay(&self, delay: Duration) {
        *self.state.delay.lock().unwrap() = delay;
    }

    pub(super) fn requests(&self) -> Vec<BTreeMap<String, String>> {
        self.state.requests.lock().unwrap().clone()
    }
}

async fn token(State(state): State<Arc<TokenState>>, body: String) -> Response {
    let form: BTreeMap<String, String> = url::form_urlencoded::parse(body.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    let refreshing = form
        .get("grant_type")
        .is_some_and(|grant| grant == "refresh_token");
    state.requests.lock().unwrap().push(form);
    let delay = *state.delay.lock().unwrap();
    tokio::time::sleep(delay).await;
    let json_type = [("content-type", "application/json")];
    if refreshing {
        let number = state
            .refreshes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let lifetime = *state.lifetime.lock().unwrap();
        return match *state.refresh.lock().unwrap() {
            RefreshReply::Rotate => (
                json_type,
                json!({"access_token": format!("owned-refreshed-access-{number}"), "token_type": "Bearer", "expires_in": lifetime, "refresh_token": format!("owned-rotated-refresh-{number}")}).to_string(),
            )
                .into_response(),
            RefreshReply::Keep => (
                json_type,
                json!({"access_token": format!("owned-refreshed-access-{number}"), "token_type": "Bearer", "expires_in": lifetime}).to_string(),
            )
                .into_response(),
            RefreshReply::InvalidGrant => (
                StatusCode::BAD_REQUEST,
                json_type,
                json!({"error": "invalid_grant", "error_description": "owned-provider-error-sentinel"}).to_string(),
            )
                .into_response(),
            RefreshReply::Unavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "owned-provider-error-sentinel").into_response()
            }
            RefreshReply::TooManyRequests => {
                (StatusCode::TOO_MANY_REQUESTS, "owned-provider-error-sentinel").into_response()
            }
            RefreshReply::Malformed => ([("content-type", "text/plain")], "owned-provider-error-sentinel").into_response(),
        };
    }
    let lifetime = *state.lifetime.lock().unwrap();
    match *state.reply.lock().unwrap() {
        Reply::Grant => (
            json_type,
            json!({"access_token": ACCESS, "token_type": "Bearer", "expires_in": lifetime, "refresh_token": REFRESH, "scope": "playlist-read-private"}).to_string(),
        )
            .into_response(),
        Reply::Rejected => (
            StatusCode::BAD_REQUEST,
            json_type,
            json!({"error": "invalid_grant", "error_description": "owned-provider-error-sentinel"}).to_string(),
        )
            .into_response(),
        Reply::Redirect => (StatusCode::TEMPORARY_REDIRECT, [("location", "/elsewhere")]).into_response(),
        Reply::Oversized => (json_type, format!("{{\"access_token\":\"{}\"}}", "a".repeat(40000))).into_response(),
        Reply::NotJson => ([("content-type", "text/plain")], ACCESS).into_response(),
        Reply::GrantStringExpiry => (
            json_type,
            json!({"access_token": ACCESS, "token_type": "bearer", "expires_in": "3600"}).to_string(),
        )
            .into_response(),
        Reply::GrantUpperIdentity => (
            [("content-type", "application/json"), ("content-encoding", "Identity")],
            json!({"access_token": ACCESS, "token_type": "Bearer"}).to_string(),
        )
            .into_response(),
        Reply::GrantAs(name) => (
            json_type,
            json!({"access_token": format!("{name}-access"), "token_type": "Bearer", "expires_in": lifetime, "refresh_token": format!("{name}-refresh"), "scope": "playlist-read-private"}).to_string(),
        )
            .into_response(),
        Reply::NoAccessToken => (json_type, json!({"token_type": "Bearer"}).to_string()).into_response(),
    }
}

pub(super) struct Attempt {
    pub(super) id: String,
    pub(super) state: String,
    pub(super) challenge: String,
}

pub(super) struct Flow {
    pub(super) app: Router,
    pub(super) directory: tempfile::TempDir,
    pub(super) owner: String,
    pub(super) installation: Value,
    pub(super) service: Value,
    pub(super) token: TokenServer,
    pub(super) provider: ProviderServer,
    pub(super) services: hito_core::Services,
}

pub(super) struct Page {
    pub(super) status: StatusCode,
    pub(super) headers: HeaderMap,
    pub(super) body: String,
}

impl Flow {
    pub(super) async fn start() -> Self {
        Self::start_with(None).await
    }

    pub(super) async fn start_with(endpoint: Option<&str>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let services = services(directory.path(), &directory.path().join("music")).await;
        let services_handle = services.clone();
        let app = hito_server::router_with_config(
            services,
            hito_server::Config {
                origin: "https://hito.test".into(),
                ..Default::default()
            },
        );
        let owner = admin(&app).await;
        let token = TokenServer::start().await;
        let provider = ProviderServer::start().await;
        provider
            .reply(
                "v1/manifest",
                json!({
                    "id": "org.hito.callback-fixture",
                    "name": "Callback fixture",
                    "protocol_version": 1,
                    "capabilities": ["playlists"],
                    "authorization": {
                        "kind": "oauth2_pkce",
                        "client_id": "owned-test-client",
                        "authorization_endpoint": "https://accounts.spotify.com/authorize",
                        "token_endpoint": endpoint.map_or_else(|| format!("{}/token", token.url), str::to_owned),
                        "scopes": ["playlist-read-private"]
                    }
                }),
            )
            .await;
        let (status, service) = call(
            &app,
            "POST",
            "/api/v1/plugins/services",
            json!({"name": "Callback fixture", "base_url": provider.url}),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, installation) = call(
            &app,
            "POST",
            "/api/v1/plugins",
            json!({"service_id": service["id"], "service_revision": service["revision"]}),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        Self {
            app,
            directory,
            owner,
            installation,
            service,
            token,
            provider,
            services: services_handle,
        }
    }

    pub(super) fn begin_path(&self) -> String {
        format!(
            "/api/v1/plugins/{}/authorization",
            self.installation["id"].as_str().unwrap()
        )
    }

    pub(super) async fn begin_as(&self, session: &str) -> Attempt {
        self.begin_for(session, &self.installation).await
    }

    pub(super) async fn begin_for(&self, session: &str, installation: &Value) -> Attempt {
        let (status, attempt) = call(
            &self.app,
            "POST",
            &format!(
                "/api/v1/plugins/{}/authorization",
                installation["id"].as_str().unwrap()
            ),
            json!({"revision": installation["revision"]}),
            Some(session),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{attempt}");
        let uri: Uri = attempt["authorization_url"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let query: BTreeMap<String, String> =
            url::form_urlencoded::parse(uri.query().unwrap().as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
        Attempt {
            id: attempt["id"].as_str().unwrap().to_owned(),
            state: query["state"].clone(),
            challenge: query["code_challenge"].clone(),
        }
    }

    pub(super) async fn connect(&self) -> Attempt {
        let attempt = self.begin().await;
        let page = self.code_callback(&self.owner, &attempt).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
        attempt
    }

    pub(super) fn connection_path(&self) -> String {
        format!(
            "/api/v1/plugins/{}/connection",
            self.installation["id"].as_str().unwrap()
        )
    }

    pub(super) async fn connection(&self, session: &str) -> (StatusCode, Value) {
        call(
            &self.app,
            "GET",
            &self.connection_path(),
            Value::Null,
            Some(session),
        )
        .await
    }

    pub(super) async fn disconnect(&self, session: &str) -> StatusCode {
        call(
            &self.app,
            "DELETE",
            &self.connection_path(),
            Value::Null,
            Some(session),
        )
        .await
        .0
    }

    pub(super) async fn actor(&self, session: &str) -> hito_core::Actor {
        self.services.identity.authenticate(session).await.unwrap()
    }

    pub(super) async fn access(&self) -> hito_core::Result<hito_core::ProviderAccess> {
        let actor = self.actor(&self.owner).await;
        self.services
            .plugins
            .access_token(&actor, self.installation["id"].as_str().unwrap())
            .await
    }

    pub(super) async fn begin(&self) -> Attempt {
        self.begin_as(&self.owner).await
    }

    pub(super) async fn callback(&self, session: Option<&str>, query: &str) -> Page {
        self.callback_with(session, query, &[]).await
    }

    pub(super) async fn callback_with(
        &self,
        session: Option<&str>,
        query: &str,
        headers: &[(&str, &str)],
    ) -> Page {
        let mut request = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/plugin-authorization/callback?{query}"));
        if let Some(session) = session {
            request = request.header("cookie", format!("hito_session={session}"));
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        Page {
            status: parts.status,
            headers: parts.headers,
            body: String::from_utf8(body.collect().await.unwrap().to_bytes().to_vec()).unwrap(),
        }
    }

    pub(super) async fn code_callback(&self, session: &str, attempt: &Attempt) -> Page {
        self.callback(
            Some(session),
            &format!("code={CODE}&state={}", attempt.state),
        )
        .await
    }

    pub(super) async fn status(&self, session: &str, attempt: &Attempt) -> (StatusCode, Value) {
        call(
            &self.app,
            "GET",
            &format!("{}/{}", self.begin_path(), attempt.id),
            Value::Null,
            Some(session),
        )
        .await
    }

    pub(super) async fn state_of(&self, session: &str, attempt: &Attempt) -> String {
        self.status(session, attempt).await.1["status"]
            .as_str()
            .unwrap_or("absent")
            .to_owned()
    }

    pub(super) async fn login(&self, username: &str, password: &str) -> String {
        let (status, session) = call(
            &self.app,
            "POST",
            "/api/v1/login",
            json!({"username": username, "password": password}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        session["token"].as_str().unwrap().to_owned()
    }

    pub(super) async fn sql(&self, statement: &str) {
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(self.directory.path().join("hito.db")),
        )
        .await
        .unwrap();
        sqlx::query(statement)
            .execute(&mut connection)
            .await
            .unwrap();
    }
}

#[derive(Clone, Default)]
pub(super) struct Capture(pub(super) Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
