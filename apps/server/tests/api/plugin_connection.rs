use super::authorization_flow::*;
use super::support::call;
use axum::http::StatusCode;
use hito_core::{Error, PluginConnectionState, ProviderAccess};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::instrument::WithSubscriber;

fn token(access: &ProviderAccess) -> String {
    access.token().to_owned()
}

async fn database_bytes(flow: &Flow) -> Vec<u8> {
    let mut bytes = Vec::new();
    for name in ["hito.db", "hito.db-wal"] {
        if let Ok(content) = std::fs::read(flow.directory.path().join(name)) {
            bytes.extend(content);
        }
    }
    bytes
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

async fn force_expiry(flow: &Flow, seconds: i64) {
    flow.sql(&format!(
        "UPDATE plugin_connections SET expires_at = unixepoch() + ({seconds})"
    ))
    .await;
}

async fn connection_rows(flow: &Flow) -> Vec<(String, bool)> {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(flow.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let rows = sqlx::query_as("SELECT status,tokens IS NOT NULL FROM plugin_connections")
        .fetch_all(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    rows
}

#[tokio::test]
async fn the_connection_status_follows_connect_refresh_and_disconnect_without_exposing_tokens() {
    let flow = Flow::start().await;
    flow.token.lifetime(30);
    let (status, before) = flow.connection(&flow.owner).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["status"], "not_connected");
    assert!(before["scopes"].as_array().unwrap().is_empty());
    println!("evidence before connect: {before}");

    flow.connect().await;
    let (status, connected) = flow.connection(&flow.owner).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(connected["status"], "connected");
    assert_eq!(connected["scopes"], json!(["playlist-read-private"]));
    assert!(connected["expires_at"].as_i64().unwrap() > 0);
    println!("evidence after connect: {connected}");

    flow.token.lifetime(3600);
    let refreshed = flow.access().await.unwrap();
    assert_eq!(token(&refreshed), "owned-refreshed-access-1");
    let (_, after_refresh) = flow.connection(&flow.owner).await;
    assert_eq!(after_refresh["status"], "connected");
    assert!(after_refresh["expires_at"].as_i64() > connected["expires_at"].as_i64());
    println!("evidence after refresh: {after_refresh}");

    for body in [&connected, &after_refresh] {
        let text = body.to_string();
        for private in [
            ACCESS,
            REFRESH,
            "owned-refreshed-access-1",
            "owned-rotated-refresh-1",
        ] {
            assert!(
                !text.contains(private),
                "status must never carry token material"
            );
        }
    }
    assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);
    let (_, after_disconnect) = flow.connection(&flow.owner).await;
    assert_eq!(after_disconnect["status"], "not_connected");
    println!("evidence after disconnect: {after_disconnect}");
}

#[tokio::test]
async fn a_successful_exchange_stores_one_encrypted_connection_and_nothing_readable_at_rest() {
    let flow = Flow::start().await;
    flow.connect().await;
    assert_eq!(
        connection_rows(&flow).await,
        vec![("connected".to_owned(), true)]
    );
    let bytes = database_bytes(&flow).await;
    for secret in [ACCESS, REFRESH, CODE] {
        assert!(
            !contains(&bytes, secret),
            "token bytes must not appear in the database file"
        );
    }
    assert_eq!(token(&flow.access().await.unwrap()), ACCESS);
    assert_eq!(flow.token.refresh_requests().len(), 0);
    let (status, page) = call(
        &flow.app,
        "GET",
        &flow.connection_path(),
        Value::Null,
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["status"], "connected");
}

#[tokio::test]
async fn status_and_disconnect_answer_with_no_store_and_require_a_session() {
    let flow = Flow::start().await;
    flow.connect().await;
    for method in ["GET", "DELETE"] {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(flow.connection_path())
            .header("authorization", format!("Bearer {}", flow.owner))
            .header("x-hito-request", "1")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(flow.app.clone(), request)
            .await
            .unwrap();
        assert_eq!(response.headers()["cache-control"], "no-store", "{method}");
    }
    for method in ["GET", "DELETE"] {
        let (status, _) = call(
            &flow.app,
            method,
            &flow.connection_path(),
            Value::Null,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method}");
    }
}

#[tokio::test]
async fn reauthorizing_replaces_the_connection_and_a_failed_attempt_keeps_the_old_one() {
    let flow = Flow::start().await;
    flow.connect().await;
    flow.token.reply(Reply::Rejected);
    let failed = flow.begin().await;
    assert_eq!(
        flow.code_callback(&flow.owner, &failed).await.status,
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(token(&flow.access().await.unwrap()), ACCESS);

    flow.token.reply(Reply::GrantAs("second"));
    flow.connect().await;
    assert_eq!(connection_rows(&flow).await.len(), 1);
    assert_eq!(token(&flow.access().await.unwrap()), "second-access");
    let bytes = database_bytes(&flow).await;
    assert!(!contains(&bytes, "second-access") && !contains(&bytes, "second-refresh"));
}

#[tokio::test]
async fn another_account_can_neither_read_nor_disconnect_nor_use_the_connection() {
    let flow = Flow::start().await;
    flow.connect().await;
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob connection test password"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bob = flow.login("bob", "bob connection test password").await;
    assert_eq!(flow.connection(&bob).await.0, StatusCode::NOT_FOUND);
    assert_eq!(flow.disconnect(&bob).await, StatusCode::NOT_FOUND);
    let actor = flow.actor(&bob).await;
    let denied = flow
        .services
        .plugins
        .access_token(&actor, flow.installation["id"].as_str().unwrap())
        .await;
    assert!(matches!(denied, Err(Error::NotFound)));
    assert_eq!(flow.connection(&flow.owner).await.1["status"], "connected");
    assert!(flow.token.requests().len() == 1);
}

#[tokio::test]
async fn an_expiring_access_token_is_refreshed_with_the_refresh_grant_and_a_fresh_one_is_not() {
    let flow = Flow::start().await;
    flow.connect().await;
    assert_eq!(token(&flow.access().await.unwrap()), ACCESS);
    assert!(flow.token.refresh_requests().is_empty());

    force_expiry(&flow, 30).await;
    let refreshed = flow.access().await.unwrap();
    assert_eq!(token(&refreshed), "owned-refreshed-access-1");
    let requests = flow.token.refresh_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["refresh_token"], REFRESH);
    assert_eq!(requests[0]["client_id"], "owned-test-client");
    assert!(!requests[0].contains_key("client_secret"));
    assert!(!requests[0].contains_key("code"));
    assert_eq!(
        token(&flow.access().await.unwrap()),
        "owned-refreshed-access-1"
    );
    assert_eq!(flow.token.refresh_requests().len(), 1);

    force_expiry(&flow, -5).await;
    assert_eq!(
        token(&flow.access().await.unwrap()),
        "owned-refreshed-access-2"
    );
}

#[tokio::test]
async fn refresh_rotation_replaces_the_refresh_token_and_its_absence_keeps_the_old_one() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.access().await.unwrap();
    force_expiry(&flow, -5).await;
    flow.access().await.unwrap();
    let requests = flow.token.refresh_requests();
    assert_eq!(requests[0]["refresh_token"], REFRESH);
    assert_eq!(requests[1]["refresh_token"], "owned-rotated-refresh-1");

    flow.token.refresh_reply(RefreshReply::Keep);
    force_expiry(&flow, -5).await;
    flow.access().await.unwrap();
    force_expiry(&flow, -5).await;
    flow.access().await.unwrap();
    let requests = flow.token.refresh_requests();
    assert_eq!(requests[2]["refresh_token"], "owned-rotated-refresh-2");
    assert_eq!(requests[3]["refresh_token"], "owned-rotated-refresh-2");
    let bytes = database_bytes(&flow).await;
    assert!(!contains(&bytes, "owned-rotated-refresh-2"));
}

#[tokio::test]
async fn concurrent_callers_of_an_expired_connection_trigger_one_refresh() {
    let flow = Arc::new(Flow::start().await);
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.delay(Duration::from_millis(300));
    let mut callers = Vec::new();
    for _ in 0..8 {
        let flow = flow.clone();
        callers.push(tokio::spawn(async move { flow.access().await }));
    }
    for caller in callers {
        let access = caller.await.unwrap().unwrap();
        assert_eq!(token(&access), "owned-refreshed-access-1");
    }
    assert_eq!(flow.token.refresh_requests().len(), 1);
}

#[tokio::test]
async fn invalid_grant_wipes_the_tokens_and_requires_reauthorization_until_the_next_consent() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.refresh_reply(RefreshReply::InvalidGrant);
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(
        flow.connection(&flow.owner).await.1["status"],
        "needs_reauthorization"
    );
    assert_eq!(
        connection_rows(&flow).await,
        vec![("needs_reauthorization".to_owned(), false)]
    );
    let calls = flow.token.requests().len();
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(
        flow.token.requests().len(),
        calls,
        "a wiped connection must not call the provider"
    );

    flow.token.reply(Reply::GrantAs("again"));
    flow.connect().await;
    assert_eq!(flow.connection(&flow.owner).await.1["status"], "connected");
    assert_eq!(token(&flow.access().await.unwrap()), "again-access");
}

#[tokio::test]
async fn an_expired_token_without_a_refresh_token_requires_reauthorization() {
    let flow = Flow::start().await;
    flow.token.reply(Reply::GrantStringExpiry);
    flow.connect().await;
    force_expiry(&flow, -5).await;
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert!(flow.token.refresh_requests().is_empty());
    assert_eq!(
        flow.connection(&flow.owner).await.1["status"],
        "needs_reauthorization"
    );
}

#[tokio::test]
async fn transient_refresh_failures_keep_the_tokens_and_report_a_retryable_error() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    for reply in [
        RefreshReply::Unavailable,
        RefreshReply::TooManyRequests,
        RefreshReply::Malformed,
    ] {
        flow.token.refresh_reply(reply);
        assert!(matches!(flow.access().await, Err(Error::Upstream(_))));
        assert_eq!(
            connection_rows(&flow).await,
            vec![("connected".to_owned(), true)]
        );
    }
    assert_eq!(flow.token.refresh_requests().len(), 3);
    flow.token.refresh_reply(RefreshReply::Rotate);
    assert_eq!(
        token(&flow.access().await.unwrap()),
        "owned-refreshed-access-4"
    );
    assert_eq!(flow.token.refresh_requests()[3]["refresh_token"], REFRESH);
}

#[tokio::test]
async fn an_unreachable_token_endpoint_keeps_the_tokens() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.stop().await;
    assert!(matches!(flow.access().await, Err(Error::Upstream(_))));
    assert_eq!(
        connection_rows(&flow).await,
        vec![("connected".to_owned(), true)]
    );
}

#[tokio::test]
async fn disconnecting_wipes_the_tokens_immediately_and_closes_pending_consent() {
    let flow = Flow::start().await;
    flow.connect().await;
    let pending = flow.begin().await;
    assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);
    assert!(connection_rows(&flow).await.is_empty());
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);
    let page = flow.code_callback(&flow.owner, &pending).await;
    assert_eq!(page.status, StatusCode::CONFLICT);
    assert!(connection_rows(&flow).await.is_empty());
    assert_eq!(flow.token.requests().len(), 1);
}

#[tokio::test]
async fn disconnecting_during_a_refresh_is_not_undone_by_the_refresh() {
    let flow = Arc::new(Flow::start().await);
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.delay(Duration::from_millis(400));
    let refreshing = {
        let flow = flow.clone();
        tokio::spawn(async move { flow.access().await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);
    assert!(matches!(
        refreshing.await.unwrap(),
        Err(Error::AuthorizationRequired(_))
    ));
    assert!(connection_rows(&flow).await.is_empty());
}

#[tokio::test]
async fn removing_the_installation_withdrawing_the_service_or_changing_the_declaration_wipes_the_connection()
 {
    let flow = Flow::start().await;
    flow.connect().await;
    let (status, _) = call(
        &flow.app,
        "DELETE",
        &format!(
            "/api/v1/plugins/{}",
            flow.installation["id"].as_str().unwrap()
        ),
        json!({"revision": flow.installation["revision"]}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(connection_rows(&flow).await.is_empty());

    let flow = Flow::start().await;
    flow.connect().await;
    let (status, _) = call(
        &flow.app,
        "DELETE",
        &format!(
            "/api/v1/plugins/services/{}",
            flow.service["id"].as_str().unwrap()
        ),
        json!({"revision": flow.service["revision"]}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(connection_rows(&flow).await.is_empty());
}

fn manifest(name: &str, token_endpoint: &str, scope: &str) -> Value {
    json!({
        "id": "org.hito.callback-fixture",
        "name": name,
        "protocol_version": 1,
        "capabilities": ["playlists"],
        "authorization": {
            "kind": "oauth2_pkce",
            "client_id": "owned-test-client",
            "authorization_endpoint": "https://accounts.spotify.com/authorize",
            "token_endpoint": token_endpoint,
            "scopes": [scope]
        }
    })
}

async fn refresh_manifest(flow: &Flow, revision: &Value) -> (StatusCode, Value) {
    let path = format!(
        "/api/v1/plugins/{}/refresh",
        flow.installation["id"].as_str().unwrap()
    );
    let (status, review) = call(
        &flow.app,
        "POST",
        &path,
        json!({"revision": revision}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(review["kind"], "review_required");
    call(
        &flow.app,
        "POST",
        &path,
        json!({"revision": revision, "accept_manifest_digest": review["digest"]}),
        Some(&flow.owner),
    )
    .await
}

#[tokio::test]
async fn a_manifest_change_that_alters_the_authorization_declaration_wipes_the_connection() {
    let flow = Flow::start().await;
    flow.connect().await;
    let endpoint = format!("{}/token", flow.token.url);
    flow.provider
        .reply(
            "v1/manifest",
            manifest("Callback fixture", &endpoint, "playlist-modify-private"),
        )
        .await;
    assert_eq!(flow.connection(&flow.owner).await.1["status"], "connected");
    let (status, updated) = refresh_manifest(&flow, &flow.installation["revision"]).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        flow.connection(&flow.owner).await.1["status"],
        "not_connected"
    );
    assert!(connection_rows(&flow).await.is_empty());
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
}

#[tokio::test]
async fn a_manifest_change_that_keeps_the_authorization_declaration_keeps_the_connection() {
    let flow = Flow::start().await;
    flow.connect().await;
    let endpoint = format!("{}/token", flow.token.url);
    flow.provider
        .reply(
            "v1/manifest",
            manifest("Renamed fixture", &endpoint, "playlist-read-private"),
        )
        .await;
    let (status, updated) = refresh_manifest(&flow, &flow.installation["revision"]).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["kind"], "updated");
    assert_eq!(flow.connection(&flow.owner).await.1["status"], "connected");
    assert_eq!(token(&flow.access().await.unwrap()), ACCESS);
}

#[tokio::test]
async fn a_declaration_changed_behind_the_connection_is_detected_when_it_is_used() {
    let flow = Flow::start().await;
    flow.connect().await;
    flow.sql("UPDATE plugin_installations SET manifest = replace(manifest, 'playlist-read-private', 'playlist-modify-private')").await;
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert!(connection_rows(&flow).await.is_empty());
}

#[tokio::test]
async fn connections_survive_a_restart_and_a_lost_key_protects_them_until_they_are_disconnected() {
    let flow = Flow::start().await;
    flow.connect().await;
    let requests = flow.token.requests().len();
    let Flow {
        app,
        directory,
        services,
        owner,
        installation,
        ..
    } = flow;
    let id = installation["id"].as_str().unwrap().to_owned();
    drop(app);
    drop(services);
    let music = directory.path().join("music");
    let key = directory.path().join("hito.plugin-key");
    let saved = std::fs::read(&key).unwrap();

    let restarted = super::support::services(directory.path(), &music).await;
    let actor = restarted.identity.authenticate(&owner).await.unwrap();
    let access = restarted.plugins.access_token(&actor, &id).await.unwrap();
    assert_eq!(token(&access), ACCESS);
    drop(restarted);

    std::fs::remove_file(&key).unwrap();
    let keyless = super::support::services(directory.path(), &music).await;
    let actor = keyless.identity.authenticate(&owner).await.unwrap();
    assert!(
        !key.exists(),
        "a live connection must not permit key replacement"
    );
    assert!(matches!(
        keyless.plugins.access_token(&actor, &id).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        keyless
            .plugins
            .connection_status(&actor, &id)
            .await
            .unwrap()
            .status,
        PluginConnectionState::Connected
    ));
    keyless.plugins.disconnect(&actor, &id).await.unwrap();
    drop(keyless);

    let recovered = super::support::services(directory.path(), &music).await;
    assert_eq!(std::fs::metadata(&key).unwrap().len(), 32);
    assert_ne!(std::fs::read(&key).unwrap(), saved);
    let actor = recovered.identity.authenticate(&owner).await.unwrap();
    assert!(matches!(
        recovered
            .plugins
            .connection_status(&actor, &id)
            .await
            .unwrap()
            .status,
        PluginConnectionState::NotConnected
    ));
    assert!(requests >= 1);
}

#[tokio::test]
async fn connection_workflows_log_outcomes_and_timings_but_never_tokens_codes_the_client_id_or_provider_endpoints()
 {
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
        flow.connect().await;
        force_expiry(&flow, -5).await;
        flow.access().await.unwrap();
        force_expiry(&flow, -5).await;
        flow.token.refresh_reply(RefreshReply::Unavailable);
        flow.access().await.unwrap_err();
        flow.token.refresh_reply(RefreshReply::InvalidGrant);
        flow.access().await.unwrap_err();
        flow.token.reply(Reply::GrantAs("logged"));
        flow.connect().await;
        flow.connection(&flow.owner).await;
        assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let entries: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for message in [
            "Provider connection refreshed",
            "Provider connection refresh failed",
            "Provider connection needs reauthorization",
            "Provider connection disconnected",
            "Provider access token served",
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry["fields"]["message"] == message)
                .unwrap_or_else(|| panic!("Missing workflow log: {message}"));
            assert!(entry["fields"]["outcome"].as_str().is_some(), "{message}");
            assert!(
                entry["fields"]["elapsed_ms"].as_u64().is_some(),
                "{message}"
            );
        }
        let own: String = entries
            .iter()
            .filter(|entry| {
                entry["target"]
                    .as_str()
                    .is_some_and(|target| target.starts_with("hito"))
            })
            .map(Value::to_string)
            .collect();
        for endpoint in [flow.token.url.as_str(), "owned-test-client"] {
            assert!(
                !own.contains(endpoint),
                "Hito logs must not carry provider endpoints or client ids"
            );
        }
        for private in [
            flow.owner.as_str(),
            CODE,
            ACCESS,
            REFRESH,
            "owned-refreshed-access-1",
            "owned-rotated-refresh-1",
            "logged-access",
            "logged-refresh",
            "owned-provider-error-sentinel",
        ] {
            assert!(
                !output.contains(private),
                "Private connection value appeared in logs"
            );
        }
    }
    .with_subscriber(subscriber)
    .await;
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn expiry(flow: &Flow) -> Option<i64> {
    flow.connection(&flow.owner).await.1["expires_at"].as_i64()
}

async fn database(flow: &Flow) -> sqlx::SqliteConnection {
    use sqlx::{Connection, sqlite::SqliteConnectOptions};
    sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(flow.directory.path().join("hito.db")),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_caller_dropped_mid_refresh_does_not_lose_the_rotated_refresh_token() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.delay(Duration::from_millis(300));
    let cancelled = tokio::time::timeout(Duration::from_millis(100), flow.access()).await;
    assert!(cancelled.is_err());
    tokio::time::sleep(Duration::from_millis(600)).await;
    flow.token.delay(Duration::ZERO);
    assert_eq!(flow.token.refresh_requests().len(), 1);
    assert_eq!(
        token(&flow.access().await.unwrap()),
        "owned-refreshed-access-1"
    );
    assert_eq!(flow.token.refresh_requests().len(), 1);
    force_expiry(&flow, -5).await;
    flow.access().await.unwrap();
    assert_eq!(
        flow.token.refresh_requests()[1]["refresh_token"],
        "owned-rotated-refresh-1"
    );
}

#[tokio::test]
async fn a_missing_lifetime_gets_a_bounded_default_and_a_zero_lifetime_the_minimum() {
    let flow = Flow::start().await;
    flow.token.reply(Reply::GrantNoExpiry);
    flow.connect().await;
    let expires = expiry(&flow).await.unwrap();
    assert!(
        (now() + 3500..=now() + 3700).contains(&expires),
        "{expires}"
    );

    flow.token.reply(Reply::GrantZeroExpiry);
    flow.connect().await;
    let expires = expiry(&flow).await.unwrap();
    assert!((now() + 100..=now() + 140).contains(&expires), "{expires}");
    assert_eq!(token(&flow.access().await.unwrap()), ACCESS);
    assert!(
        flow.token.refresh_requests().is_empty(),
        "a zero lifetime must not cause churn"
    );

    force_expiry(&flow, -5).await;
    flow.token.refresh_reply(RefreshReply::NoExpiry);
    flow.access().await.unwrap();
    let expires = expiry(&flow).await.unwrap();
    assert!(
        (now() + 3500..=now() + 3700).contains(&expires),
        "{expires}"
    );

    force_expiry(&flow, -5).await;
    flow.token.refresh_reply(RefreshReply::ZeroExpiry);
    flow.access().await.unwrap();
    let expires = expiry(&flow).await.unwrap();
    assert!((now() + 100..=now() + 140).contains(&expires), "{expires}");
    flow.access().await.unwrap();
    assert_eq!(flow.token.refresh_requests().len(), 2);

    flow.token.reply(Reply::GrantUpperIdentity);
    flow.connect().await;
    assert_eq!(expiry(&flow).await, None);
}

#[tokio::test]
async fn client_rejections_and_error_bodies_are_terminal() {
    for reply in [
        RefreshReply::InvalidClient,
        RefreshReply::Unauthorized,
        RefreshReply::ErrorOk,
    ] {
        let flow = Flow::start().await;
        flow.connect().await;
        force_expiry(&flow, -5).await;
        flow.token.refresh_reply(reply);
        assert!(matches!(
            flow.access().await,
            Err(Error::AuthorizationRequired(_))
        ));
        assert_eq!(
            connection_rows(&flow).await,
            vec![("needs_reauthorization".to_owned(), false)]
        );
    }
}

#[tokio::test]
async fn repeated_unclassified_rejections_end_in_reauthorization_but_a_success_resets_them() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.refresh_reply(RefreshReply::PlainBadRequest);
    for _ in 0..2 {
        assert!(matches!(flow.access().await, Err(Error::Upstream(_))));
    }
    flow.token.refresh_reply(RefreshReply::Rotate);
    flow.access().await.unwrap();
    force_expiry(&flow, -5).await;
    flow.token.refresh_reply(RefreshReply::PlainBadRequest);
    for _ in 0..2 {
        assert!(matches!(flow.access().await, Err(Error::Upstream(_))));
    }
    assert_eq!(
        connection_rows(&flow).await,
        vec![("connected".to_owned(), true)]
    );
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(
        connection_rows(&flow).await,
        vec![("needs_reauthorization".to_owned(), false)]
    );
}

#[tokio::test]
async fn a_day_of_unclassified_rejections_ends_in_reauthorization() {
    let flow = Flow::start().await;
    flow.connect().await;
    force_expiry(&flow, -5).await;
    flow.token.refresh_reply(RefreshReply::PlainBadRequest);
    assert!(matches!(flow.access().await, Err(Error::Upstream(_))));
    flow.sql("UPDATE plugin_connections SET refresh_failing_since = unixepoch() - 90000")
        .await;
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
}

#[tokio::test]
async fn a_stale_refresh_never_overwrites_or_invalidates_a_newer_consent() {
    for reply in [RefreshReply::Rotate, RefreshReply::InvalidGrant] {
        let flow = Arc::new(Flow::start().await);
        flow.connect().await;
        force_expiry(&flow, -5).await;
        flow.token.refresh_reply(reply);
        flow.token.delay(Duration::from_millis(400));
        let refreshing = {
            let flow = flow.clone();
            tokio::spawn(async move { flow.access().await })
        };
        tokio::time::sleep(Duration::from_millis(150)).await;
        flow.token.delay(Duration::ZERO);
        flow.token.reply(Reply::GrantAs("second"));
        flow.connect().await;
        let served = refreshing.await.unwrap().unwrap();
        assert_eq!(token(&served), "second-access");
        assert_eq!(flow.connection(&flow.owner).await.1["status"], "connected");
        assert_eq!(token(&flow.access().await.unwrap()), "second-access");
    }
}

#[tokio::test]
async fn two_installations_of_one_account_refresh_concurrently_and_stay_isolated() {
    let flow = Flow::start().await;
    flow.connect().await;
    let (_second_provider, second) = flow.second_installation().await;
    flow.token.reply(Reply::GrantAs("two"));
    flow.connect_installation(&flow.owner, &second).await;
    assert_eq!(
        token(&flow.access_for(&second).await.unwrap()),
        "two-access"
    );
    assert_eq!(token(&flow.access().await.unwrap()), ACCESS);

    force_expiry(&flow, -5).await;
    flow.token.delay(Duration::from_millis(300));
    let started = Instant::now();
    let (first, other) = tokio::join!(flow.access(), flow.access_for(&second));
    assert!(first.is_ok() && other.is_ok());
    assert!(
        started.elapsed() < Duration::from_millis(550),
        "refreshes must overlap"
    );
    assert_eq!(flow.token.refresh_requests().len(), 2);
    flow.token.delay(Duration::ZERO);

    assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert!(flow.access_for(&second).await.is_ok());
    assert_eq!(connection_rows(&flow).await.len(), 1);
}

#[tokio::test]
async fn tokens_moved_to_another_installation_cannot_be_read_and_force_reauthorization() {
    use sqlx::Connection;
    let flow = Flow::start().await;
    flow.connect().await;
    let (_second_provider, second) = flow.second_installation().await;
    flow.token.reply(Reply::GrantAs("two"));
    flow.connect_installation(&flow.owner, &second).await;
    let mut connection = database(&flow).await;
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT installation_id,tokens FROM plugin_connections ORDER BY installation_id",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    for (index, (id, _)) in rows.iter().enumerate() {
        sqlx::query("UPDATE plugin_connections SET tokens = ? WHERE installation_id = ?")
            .bind(&rows[1 - index].1)
            .bind(id)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    connection.close().await.unwrap();
    assert!(matches!(
        flow.access().await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert!(matches!(
        flow.access_for(&second).await,
        Err(Error::AuthorizationRequired(_))
    ));
    assert_eq!(
        connection_rows(&flow).await,
        vec![("needs_reauthorization".to_owned(), false); 2]
    );
}

#[tokio::test]
async fn deleting_an_account_with_a_live_connection_removes_it() {
    let flow = Flow::start().await;
    let (status, bob) = call(
        &flow.app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob connection test password"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let session = flow.login("bob", "bob connection test password").await;
    let installation = flow.install_as(&session).await;
    flow.connect_installation(&session, &installation).await;
    assert_eq!(
        connection_rows(&flow).await,
        vec![("connected".to_owned(), true)]
    );
    let (status, _) = call(
        &flow.app,
        "DELETE",
        &format!("/api/v1/users/{}", bob["id"].as_str().unwrap()),
        json!({"username": "bob"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(connection_rows(&flow).await.is_empty());
}
