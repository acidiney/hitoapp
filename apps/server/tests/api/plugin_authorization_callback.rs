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
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::BTreeMap,
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

const CODE: &str = "owned-provider-code-sentinel";
const ACCESS: &str = "owned-access-token-sentinel";
const REFRESH: &str = "owned-refresh-token-sentinel";

#[derive(Clone, Copy)]
enum Reply {
    Grant,
    Rejected,
    Redirect,
    Oversized,
    NotJson,
    NoAccessToken,
}

struct TokenState {
    requests: Mutex<Vec<BTreeMap<String, String>>>,
    reply: Mutex<Reply>,
    delay: Mutex<Duration>,
}

struct TokenServer {
    url: String,
    state: Arc<TokenState>,
}

impl TokenServer {
    async fn start() -> Self {
        let state = Arc::new(TokenState {
            requests: Mutex::default(),
            reply: Mutex::new(Reply::Grant),
            delay: Mutex::new(Duration::ZERO),
        });
        let router = Router::new()
            .route("/token", axum::routing::post(token))
            .route("/elsewhere", axum::routing::post(|| async { "moved" }))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, state }
    }

    fn reply(&self, reply: Reply) {
        *self.state.reply.lock().unwrap() = reply;
    }

    fn delay(&self, delay: Duration) {
        *self.state.delay.lock().unwrap() = delay;
    }

    fn requests(&self) -> Vec<BTreeMap<String, String>> {
        self.state.requests.lock().unwrap().clone()
    }
}

async fn token(State(state): State<Arc<TokenState>>, body: String) -> Response {
    state.requests.lock().unwrap().push(
        url::form_urlencoded::parse(body.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect(),
    );
    let delay = *state.delay.lock().unwrap();
    tokio::time::sleep(delay).await;
    let json_type = [("content-type", "application/json")];
    match *state.reply.lock().unwrap() {
        Reply::Grant => (
            json_type,
            json!({"access_token": ACCESS, "token_type": "Bearer", "expires_in": 3600, "refresh_token": REFRESH, "scope": "playlist-read-private"}).to_string(),
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
        Reply::NoAccessToken => (json_type, json!({"token_type": "Bearer"}).to_string()).into_response(),
    }
}

struct Attempt {
    id: String,
    state: String,
    challenge: String,
}

struct Flow {
    app: Router,
    directory: tempfile::TempDir,
    owner: String,
    installation: Value,
    token: TokenServer,
    _provider: ProviderServer,
}

struct Page {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Flow {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let services = services(directory.path(), &directory.path().join("music")).await;
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
                        "token_endpoint": format!("{}/token", token.url),
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
            token,
            _provider: provider,
        }
    }

    fn begin_path(&self) -> String {
        format!(
            "/api/v1/plugins/{}/authorization",
            self.installation["id"].as_str().unwrap()
        )
    }

    async fn begin_as(&self, session: &str) -> Attempt {
        let (status, attempt) = call(
            &self.app,
            "POST",
            &self.begin_path(),
            json!({"revision": self.installation["revision"]}),
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

    async fn begin(&self) -> Attempt {
        self.begin_as(&self.owner).await
    }

    async fn callback(&self, session: Option<&str>, query: &str) -> Page {
        self.callback_with(session, query, &[]).await
    }

    async fn callback_with(
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

    async fn code_callback(&self, session: &str, attempt: &Attempt) -> Page {
        self.callback(
            Some(session),
            &format!("code={CODE}&state={}", attempt.state),
        )
        .await
    }

    async fn status(&self, session: &str, attempt: &Attempt) -> (StatusCode, Value) {
        call(
            &self.app,
            "GET",
            &format!("{}/{}", self.begin_path(), attempt.id),
            Value::Null,
            Some(session),
        )
        .await
    }

    async fn state_of(&self, session: &str, attempt: &Attempt) -> String {
        self.status(session, attempt).await.1["status"]
            .as_str()
            .unwrap_or("absent")
            .to_owned()
    }

    async fn login(&self, username: &str, password: &str) -> String {
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

    async fn sql(&self, statement: &str) {
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

fn assert_safe(page: &Page, attempt: &Attempt) {
    assert_eq!(page.headers["cache-control"], "no-store");
    assert!(
        page.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    for secret in [
        CODE,
        ACCESS,
        REFRESH,
        attempt.state.as_str(),
        attempt.challenge.as_str(),
        "owned-provider-error-sentinel",
    ] {
        assert!(
            !page.body.contains(secret),
            "The callback page must not reflect private values"
        );
    }
}

fn s256(verifier: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[tokio::test]
async fn callback_exchanges_the_code_once_with_the_stored_verifier_and_marks_the_attempt_completed()
{
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let page = flow.code_callback(&flow.owner, &attempt).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Provider authorized"));
    assert_safe(&page, &attempt);
    let requests = flow.token.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["grant_type"], "authorization_code");
    assert_eq!(requests[0]["code"], CODE);
    assert_eq!(requests[0]["client_id"], "owned-test-client");
    assert_eq!(
        requests[0]["redirect_uri"],
        "https://hito.test/api/v1/plugin-authorization/callback"
    );
    assert_eq!(s256(&requests[0]["code_verifier"]), attempt.challenge);
    assert!(!requests[0].contains_key("client_secret"));
    let (status, completed) = flow.status(&flow.owner, &attempt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["status"], "completed");
    println!("callback page: {} {}", page.status, page.body);
    println!("attempt status: {completed}");
}

#[tokio::test]
async fn wrong_missing_malformed_and_duplicate_state_are_rejected_without_consuming_the_attempt() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let mut other = attempt.state.clone();
    other.replace_range(0..1, if other.starts_with('A') { "B" } else { "A" });
    for query in [
        format!("code={CODE}&state={other}"),
        format!("code={CODE}"),
        format!("code={CODE}&state="),
        format!("code={CODE}&state={0}&state={0}", attempt.state),
        format!("code={CODE}&state={0}&code={CODE}", attempt.state),
        format!("code={CODE}&state={}", "a".repeat(129)),
        format!("state={}", attempt.state),
    ] {
        let page = flow.callback(Some(&flow.owner), &query).await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST, "{query}");
        assert_safe(&page, &attempt);
    }
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "pending");
    assert_eq!(
        flow.code_callback(&flow.owner, &attempt).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn expired_attempt_is_refused_without_an_exchange() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    flow.sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1").await;
    let page = flow.code_callback(&flow.owner, &attempt).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(page.body.contains("expired"));
    assert_safe(&page, &attempt);
    assert!(flow.token.requests().is_empty());
    assert_eq!(
        flow.status(&flow.owner, &attempt).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn another_session_or_account_cannot_complete_the_attempt() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let second_session = flow.login("admin", "administrator test password").await;
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob callback test password"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let other_account = flow.login("bob", "bob callback test password").await;
    for session in [&second_session, &other_account] {
        let page = flow.code_callback(session, &attempt).await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST);
        assert_safe(&page, &attempt);
    }
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "pending");
    assert_eq!(
        flow.code_callback(&flow.owner, &attempt).await.status,
        StatusCode::OK
    );
    assert_eq!(flow.token.requests().len(), 1);
}

#[tokio::test]
async fn changed_installation_or_service_revision_closes_the_attempt_without_an_exchange() {
    for statement in [
        "UPDATE plugin_installations SET revision = revision + 1",
        "UPDATE plugin_services SET revision = revision + 1",
    ] {
        let flow = Flow::start().await;
        let attempt = flow.begin().await;
        flow.sql(statement).await;
        let page = flow.code_callback(&flow.owner, &attempt).await;
        assert_eq!(page.status, StatusCode::CONFLICT, "{statement}");
        assert_safe(&page, &attempt);
        assert!(flow.token.requests().is_empty());
        let replay = flow.code_callback(&flow.owner, &attempt).await;
        assert_eq!(replay.status, StatusCode::CONFLICT);
        assert!(flow.token.requests().is_empty());
    }
}

#[tokio::test]
async fn replay_after_success_fails_safely_with_the_same_or_a_different_code() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    assert_eq!(
        flow.code_callback(&flow.owner, &attempt).await.status,
        StatusCode::OK
    );
    for code in [CODE, "owned-different-code-sentinel"] {
        let page = flow
            .callback(
                Some(&flow.owner),
                &format!("code={code}&state={}", attempt.state),
            )
            .await;
        assert_eq!(page.status, StatusCode::CONFLICT);
        assert!(page.body.contains("already used"));
        assert_safe(&page, &attempt);
    }
    assert_eq!(flow.token.requests().len(), 1);
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "completed");
}

#[tokio::test]
async fn concurrent_duplicate_callbacks_perform_exactly_one_exchange() {
    let flow = Flow::start().await;
    flow.token.delay(Duration::from_millis(400));
    let attempt = flow.begin().await;
    let (first, second) = tokio::join!(
        flow.code_callback(&flow.owner, &attempt),
        flow.code_callback(&flow.owner, &attempt)
    );
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    assert_eq!(flow.token.requests().len(), 1);
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "completed");
}

#[tokio::test]
async fn a_second_callback_during_the_exchange_is_refused_and_the_status_shows_exchanging() {
    let flow = Flow::start().await;
    flow.token.delay(Duration::from_millis(600));
    let attempt = flow.begin().await;
    let running = flow.code_callback(&flow.owner, &attempt);
    let observer = async {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let state = flow.state_of(&flow.owner, &attempt).await;
        let duplicate = flow.code_callback(&flow.owner, &attempt).await;
        (state, duplicate.status)
    };
    let (page, (state, duplicate)) = tokio::join!(running, observer);
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(state, "exchanging");
    assert_eq!(duplicate, StatusCode::CONFLICT);
    assert_eq!(flow.token.requests().len(), 1);
}

#[tokio::test]
async fn provider_error_parameter_closes_the_attempt_safely_without_reflection_or_exchange() {
    let flow = Flow::start().await;
    let denied = flow.begin().await;
    let page = flow
        .callback(
            Some(&flow.owner),
            &format!(
                "error=access_denied&state={}&error_description=owned-provider-error-sentinel",
                denied.state
            ),
        )
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("declined"));
    assert_safe(&page, &denied);
    let other = flow.begin().await;
    let page = flow
        .callback(
            Some(&flow.owner),
            &format!(
                "error=%3Cscript%3Ealert(1)%3C%2Fscript%3E&state={}",
                other.state
            ),
        )
        .await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(!page.body.contains("<script>"));
    assert_safe(&page, &other);
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &denied).await, "failed");
    assert_eq!(flow.state_of(&flow.owner, &other).await, "failed");
    let forged = flow
        .callback(
            Some(&flow.owner),
            "error=access_denied&state=owned-forged-state-value-0000000000000000",
        )
        .await;
    assert_eq!(forged.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn failed_exchanges_free_their_slots_and_a_new_attempt_can_succeed() {
    for reply in [
        Reply::Rejected,
        Reply::Redirect,
        Reply::Oversized,
        Reply::NotJson,
        Reply::NoAccessToken,
    ] {
        let flow = Flow::start().await;
        flow.token.reply(reply);
        for _ in 0..8 {
            let attempt = flow.begin().await;
            let page = flow.code_callback(&flow.owner, &attempt).await;
            assert_eq!(page.status, StatusCode::BAD_GATEWAY);
            assert!(page.body.contains("could not finish"));
            assert_safe(&page, &attempt);
            assert_eq!(flow.state_of(&flow.owner, &attempt).await, "failed");
            let replay = flow.code_callback(&flow.owner, &attempt).await;
            assert_eq!(replay.status, StatusCode::CONFLICT);
        }
        flow.token.reply(Reply::Grant);
        let fresh = flow.begin().await;
        assert_eq!(
            flow.code_callback(&flow.owner, &fresh).await.status,
            StatusCode::OK
        );
        assert_eq!(flow.state_of(&flow.owner, &fresh).await, "completed");
        assert_eq!(flow.token.requests().len(), 9);
    }
}

#[tokio::test]
async fn logout_before_or_during_the_exchange_leaves_nothing_connected() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/logout",
        Value::Null,
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let page = flow.code_callback(&flow.owner, &attempt).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert_safe(&page, &attempt);
    let fresh_login = flow.login("admin", "administrator test password").await;
    let page = flow.code_callback(&fresh_login, &attempt).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(flow.token.requests().is_empty());

    flow.token.delay(Duration::from_millis(500));
    let running = flow.begin_as(&fresh_login).await;
    let logging_out = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        call(
            &flow.app,
            "POST",
            "/api/v1/logout",
            Value::Null,
            Some(&fresh_login),
        )
        .await
        .0
    };
    let (page, logout) = tokio::join!(flow.code_callback(&fresh_login, &running), logging_out);
    assert_eq!(logout, StatusCode::NO_CONTENT);
    assert_eq!(page.status, StatusCode::CONFLICT);
    assert_safe(&page, &running);
    assert_eq!(flow.token.requests().len(), 1);
    let again = flow.login("admin", "administrator test password").await;
    assert_eq!(flow.status(&again, &running).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cross_site_arrival_without_a_session_cookie_gets_a_same_origin_retry_page() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let query = format!("code={CODE}&state={}", attempt.state);
    let page = flow
        .callback_with(None, &query, &[("sec-fetch-site", "cross-site")])
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("http-equiv=\"refresh\""));
    assert_safe(&page, &attempt);
    let retry = flow
        .callback_with(None, &query, &[("sec-fetch-site", "same-origin")])
        .await;
    assert_eq!(retry.status, StatusCode::UNAUTHORIZED);
    assert!(!retry.body.contains("http-equiv"));
    assert_safe(&retry, &attempt);
    let direct = flow.callback(None, &query).await;
    assert_eq!(direct.status, StatusCode::UNAUTHORIZED);
    assert!(flow.token.requests().is_empty());
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn callback_logs_outcomes_without_state_codes_verifiers_tokens_or_provider_bodies() {
    hito_testkit::logs::anchor_callsite_interest();
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        let flow = Flow::start().await;
        let good = flow.begin().await;
        assert_eq!(
            flow.code_callback(&flow.owner, &good).await.status,
            StatusCode::OK
        );
        assert_eq!(
            flow.code_callback(&flow.owner, &good).await.status,
            StatusCode::CONFLICT
        );
        flow.token.reply(Reply::Rejected);
        let bad = flow.begin().await;
        assert_eq!(
            flow.code_callback(&flow.owner, &bad).await.status,
            StatusCode::BAD_GATEWAY
        );
        let verifier = flow.token.requests()[0]["code_verifier"].clone();
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let entries: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for message in [
            "Provider authorization code exchanged",
            "Provider authorization callback rejected or closed",
            "Provider authorization code exchange failed",
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry["fields"]["message"] == message)
                .unwrap_or_else(|| panic!("Missing workflow log: {message}"));
            assert!(entry["fields"]["outcome"].as_str().is_some());
            assert!(entry["fields"]["elapsed_ms"].as_u64().is_some());
        }
        for private in [
            flow.owner.as_str(),
            CODE,
            ACCESS,
            REFRESH,
            "owned-provider-error-sentinel",
            good.state.as_str(),
            good.challenge.as_str(),
            bad.state.as_str(),
            verifier.as_str(),
            "owned-test-client",
        ] {
            assert!(
                !output.contains(private),
                "Private authorization value appeared in logs"
            );
        }
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn closed_attempts_keep_no_secret_and_do_not_block_key_recovery_after_restart() {
    let flow = Flow::start().await;
    let completed = flow.begin().await;
    assert_eq!(
        flow.code_callback(&flow.owner, &completed).await.status,
        StatusCode::OK
    );
    flow.token.reply(Reply::Rejected);
    let failed = flow.begin().await;
    assert_eq!(
        flow.code_callback(&flow.owner, &failed).await.status,
        StatusCode::BAD_GATEWAY
    );
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(flow.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let secrets: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT status,pending_secret FROM plugin_authorization_attempts ORDER BY status",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        secrets.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["completed", "failed"]
    );
    assert!(
        secrets
            .iter()
            .all(|row| row.1.iter().all(|byte| *byte == 0))
    );
    connection.close().await.unwrap();

    let Flow { app, directory, .. } = flow;
    drop(app);
    let key = directory.path().join("hito.plugin-key");
    std::fs::remove_file(&key).unwrap();
    let reopened = services(directory.path(), &directory.path().join("music")).await;
    drop(reopened);
    assert_eq!(std::fs::metadata(&key).unwrap().len(), 32);
}
