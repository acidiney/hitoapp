use super::authorization_flow::*;
use super::support::call;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use hito_server::{HostBrowser, HostOpen, HostReturn};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const RETURN: &str = "http://127.0.0.1:47321/api/v1/plugin-authorization/callback";

struct Stub {
    ready: Mutex<Option<String>>,
    prepared: std::sync::atomic::AtomicUsize,
}

impl Stub {
    fn ready() -> Arc<Self> {
        Arc::new(Self {
            ready: Mutex::new(Some(RETURN.into())),
            prepared: Default::default(),
        })
    }

    fn occupied() -> Arc<Self> {
        Arc::new(Self {
            ready: Mutex::new(None),
            prepared: Default::default(),
        })
    }
}

impl HostBrowser for Stub {
    fn open(&self, _: url::Url) -> BoxFuture<'_, HostOpen> {
        Box::pin(async { HostOpen::Opened })
    }

    fn prepare_return(&self) -> BoxFuture<'_, HostReturn> {
        Box::pin(async {
            self.prepared
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match self.ready.lock().unwrap().clone() {
                Some(url) => HostReturn::Ready(url.parse().unwrap()),
                None => HostReturn::Unavailable(
                    "Port 47321 is in use, so Hito cannot receive the provider's sign-in.".into(),
                ),
            }
        })
    }
}

async fn arrive(flow: &Flow, method: &str, target: &str, cookie: Option<&str>) -> Page {
    let mut request = Request::builder().method(method).uri(target);
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    let response = flow
        .loopback
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

async fn system_browser(flow: &Flow, attempt: &Attempt) -> Page {
    arrive(
        flow,
        "GET",
        &format!(
            "/api/v1/plugin-authorization/callback?code={CODE}&state={}",
            attempt.state
        ),
        None,
    )
    .await
}

fn assert_page_is_safe(page: &Page, attempt: &Attempt) {
    assert_eq!(page.headers["cache-control"], "no-store");
    assert_eq!(page.headers["referrer-policy"], "no-referrer");
    assert!(page.headers.contains_key("content-security-policy"));
    for private in [CODE, attempt.state.as_str(), attempt.challenge.as_str()] {
        assert!(!page.body.contains(private));
    }
    assert!(!page.body.contains("href="));
}

async fn second_account(flow: &Flow) -> String {
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob loopback test password"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    flow.login("bob", "bob loopback test password").await
}

#[tokio::test]
async fn a_desktop_attempt_completes_from_the_system_browser_without_a_session_cookie() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    let page = system_browser(&flow, &attempt).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("Provider authorized"));
    assert_page_is_safe(&page, &attempt);
    let requests = flow.token.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["redirect_uri"], RETURN);
    assert_eq!(requests[0]["code"], CODE);
    assert!(requests[0]["code_verifier"].len() >= 43);
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "completed");
    let (_, connection) = flow.connection(&flow.owner).await;
    assert_eq!(connection["status"], "connected");
    println!("loopback page: {} {}", page.status, page.body.len());
    println!(
        "token request redirect_uri: {}",
        requests[0]["redirect_uri"]
    );
}

#[tokio::test]
async fn the_provider_receives_the_fixed_return_address_for_desktop_attempts() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let (status, started) = call(
        &flow.app,
        "POST",
        &flow.begin_path(),
        json!({"revision": flow.installation["revision"]}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let url: url::Url = started["authorization_url"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let redirect = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(redirect, RETURN);
}

#[tokio::test]
async fn a_guessed_state_completes_nothing() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    for guess in [
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "a",
        &"b".repeat(128),
    ] {
        let page = arrive(
            &flow,
            "GET",
            &format!("/api/v1/plugin-authorization/callback?code={CODE}&state={guess}"),
            None,
        )
        .await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST);
        assert_page_is_safe(&page, &attempt);
    }
    for query in ["", "?code=x", "?state=", "?state=a&state=b&code=x"] {
        let page = arrive(
            &flow,
            "GET",
            &format!("/api/v1/plugin-authorization/callback{query}"),
            None,
        )
        .await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST, "{query}");
    }
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "pending");
}

#[tokio::test]
async fn replaying_the_callback_does_not_exchange_again() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    assert_eq!(system_browser(&flow, &attempt).await.status, StatusCode::OK);
    let replay = system_browser(&flow, &attempt).await;
    assert_eq!(replay.status, StatusCode::CONFLICT);
    assert!(replay.body.contains("already used"));
    assert_eq!(flow.token.requests().len(), 1);
}

#[tokio::test]
async fn a_state_completes_only_the_account_that_started_it() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let bob = second_account(&flow).await;
    let bob_installation = flow.install_as(&bob).await;
    let owners = flow.begin().await;
    let bobs = flow.begin_for(&bob, &bob_installation).await;
    assert_eq!(system_browser(&flow, &owners).await.status, StatusCode::OK);
    assert_eq!(flow.token.requests().len(), 1);
    let (_, owner_connection) = flow.connection(&flow.owner).await;
    assert_eq!(owner_connection["status"], "connected");
    let bob_status = call(
        &flow.app,
        "GET",
        &format!(
            "/api/v1/plugins/{}/authorization/{}",
            bob_installation["id"].as_str().unwrap(),
            bobs.id
        ),
        Value::Null,
        Some(&bob),
    )
    .await
    .1;
    assert_eq!(bob_status["status"], "pending");
    let (_, bob_connection) = call(
        &flow.app,
        "GET",
        &format!(
            "/api/v1/plugins/{}/connection",
            bob_installation["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&bob),
    )
    .await;
    assert_eq!(bob_connection["status"], "not_connected");
    assert_eq!(system_browser(&flow, &bobs).await.status, StatusCode::OK);
    let (_, bob_connection) = call(
        &flow.app,
        "GET",
        &format!(
            "/api/v1/plugins/{}/connection",
            bob_installation["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&bob),
    )
    .await;
    assert_eq!(bob_connection["status"], "connected");
}

#[tokio::test]
async fn a_browser_attempt_is_refused_at_the_loopback_listener_and_still_completes_in_its_browser()
{
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let page = system_browser(&flow, &attempt).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "pending");
    assert_eq!(
        flow.code_callback(&flow.owner, &attempt).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_desktop_attempt_is_refused_by_the_session_callback_even_with_the_right_session() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    let page = flow.code_callback(&flow.owner, &attempt).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "pending");
    assert_eq!(system_browser(&flow, &attempt).await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_attempt_whose_session_ended_fails_safely() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let session = flow.login("admin", "administrator test password").await;
    let attempt = flow.begin_as(&session).await;
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/logout",
        Value::Null,
        Some(&session),
    )
    .await;
    assert!(status.is_success());
    let page = system_browser(&flow, &attempt).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert_eq!(page.headers["referrer-policy"], "no-referrer");
    assert!(flow.token.requests().is_empty());
    let (_, connection) = flow.connection(&flow.owner).await;
    assert_eq!(connection["status"], "not_connected");
}

#[tokio::test]
async fn a_locked_desktop_keeps_the_attempt_until_it_is_unlocked() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    let (status, _) = call(
        &flow.app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password": "administrator test password", "pin": "1234", "idle_minutes": 0}),
        Some(&flow.owner),
    )
    .await;
    assert!(status.is_success(), "{status}");
    call(
        &flow.app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&flow.owner),
    )
    .await;
    let page = system_browser(&flow, &attempt).await;
    assert_eq!(page.status, StatusCode::LOCKED);
    assert_page_is_safe(&page, &attempt);
    assert!(flow.token.requests().is_empty());
    call(
        &flow.app,
        "POST",
        "/api/v1/account/unlock",
        json!({"pin": "1234"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "pending");
    assert_eq!(system_browser(&flow, &attempt).await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_expired_desktop_attempt_is_refused() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    flow.sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1").await;
    let page = system_browser(&flow, &attempt).await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(page.body.contains("expired"));
    assert!(flow.token.requests().is_empty());
}

#[tokio::test]
async fn a_denied_sign_in_closes_the_attempt_without_an_exchange() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    let page = arrive(
        &flow,
        "GET",
        &format!(
            "/api/v1/plugin-authorization/callback?error=access_denied&state={}",
            attempt.state
        ),
        None,
    )
    .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("declined"));
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "failed");
}

#[tokio::test]
async fn the_loopback_listener_serves_only_the_callback() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    for target in [
        "/",
        "/library",
        "/settings",
        "/api/v1/plugins",
        "/api/v1/login",
        "/api/v1/plugin-authorization/callback/extra",
        "/assets/app.js",
        "/__desktop/start",
    ] {
        let page = arrive(&flow, "GET", target, Some("hito_session=anything")).await;
        assert_eq!(page.status, StatusCode::NOT_FOUND, "{target}");
        assert!(page.body.is_empty(), "{target}");
    }
    for method in ["POST", "PUT", "DELETE"] {
        let page = arrive(
            &flow,
            method,
            "/api/v1/plugin-authorization/callback?state=a",
            None,
        )
        .await;
        assert!(
            [StatusCode::FORBIDDEN, StatusCode::METHOD_NOT_ALLOWED].contains(&page.status),
            "{method} {}",
            page.status
        );
    }
}

#[tokio::test]
async fn an_unavailable_return_address_fails_the_connect_action_with_a_plain_message() {
    let flow = Flow::start_desktop(Stub::occupied()).await;
    let (status, body) = call(
        &flow.app,
        "POST",
        &flow.begin_path(),
        json!({"revision": flow.installation["revision"]}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Port 47321 is in use")
    );
    let count: i64 = {
        use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(flow.directory.path().join("hito.db")),
        )
        .await
        .unwrap();
        sqlx::query_scalar("SELECT count(*) FROM plugin_authorization_attempts")
            .fetch_one(&mut connection)
            .await
            .unwrap()
    };
    assert_eq!(count, 0);
}

#[tokio::test]
async fn the_return_address_is_prepared_only_for_an_attempt_that_passes_validation() {
    let stub = Stub::ready();
    let flow = Flow::start_desktop(stub.clone()).await;
    let revision = flow.installation["revision"].as_i64().unwrap();
    let stale = call(
        &flow.app,
        "POST",
        &flow.begin_path(),
        json!({"revision": revision + 1}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(stale.0, StatusCode::CONFLICT);
    let unknown = call(
        &flow.app,
        "POST",
        "/api/v1/plugins/no-such-installation/authorization",
        json!({"revision": revision}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(unknown.0, StatusCode::NOT_FOUND);
    assert_eq!(stub.prepared.load(std::sync::atomic::Ordering::SeqCst), 0);
    let (status, _) = call(
        &flow.app,
        "POST",
        &flow.begin_path(),
        json!({"revision": revision}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(stub.prepared.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unexpected_failure_shows_a_styled_page_with_no_referrer() {
    let flow = Flow::start_desktop(Stub::ready()).await;
    let attempt = flow.begin().await;
    flow.sql("DROP TABLE plugin_connections").await;
    let page = system_browser(&flow, &attempt).await;
    assert!(page.status.is_server_error() || page.status.is_client_error());
    assert_eq!(page.headers["referrer-policy"], "no-referrer");
    assert_eq!(page.headers["cache-control"], "no-store");
    assert!(
        page.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert!(page.body.contains("data-tone="));
}
