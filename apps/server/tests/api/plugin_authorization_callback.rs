use super::authorization_flow::*;
use super::support::{call, services};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;
use tracing::instrument::WithSubscriber;

fn assert_safe(page: &Page, attempt: &Attempt) {
    assert_eq!(page.headers["cache-control"], "no-store");
    assert_eq!(page.headers["referrer-policy"], "no-referrer");
    assert!(
        page.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let policy = page.headers["content-security-policy"].to_str().unwrap();
    assert!(policy.contains("script-src 'self';") && !policy.contains("unsafe-eval"));
    assert!(policy.contains("default-src 'self'") && policy.contains("form-action 'self'"));
    for external in ["<script", "src=", "http://", "https://", "@import", "url("] {
        assert!(
            !page.body.contains(external),
            "The callback page must be static and load nothing"
        );
    }
    assert!(page.body.contains("data-tone="));
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
    assert_eq!(statuses, [StatusCode::OK, StatusCode::SERVICE_UNAVAILABLE]);
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
    assert_eq!(duplicate, StatusCode::SERVICE_UNAVAILABLE);
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
    assert_eq!(flow.disconnect(&flow.owner).await, StatusCode::NO_CONTENT);

    let Flow {
        app,
        directory,
        services: held,
        ..
    } = flow;
    drop(held);
    drop(app);
    let key = directory.path().join("hito.plugin-key");
    std::fs::remove_file(&key).unwrap();
    let reopened = services(directory.path(), &directory.path().join("music")).await;
    drop(reopened);
    assert_eq!(std::fs::metadata(&key).unwrap().len(), 32);
}

#[tokio::test]
async fn one_account_cannot_hold_two_exchange_slots_or_starve_another_account() {
    let flow = Flow::start().await;
    flow.token.delay(Duration::from_millis(700));
    let first = flow.begin().await;
    let second = flow.begin().await;
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob fairness test password"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bob = flow.login("bob", "bob fairness test password").await;
    let (status, bob_installation) = call(
        &flow.app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": flow.service["id"], "service_revision": flow.service["revision"]}),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bobs = flow.begin_for(&bob, &bob_installation).await;
    let stalled = flow.code_callback(&flow.owner, &first);
    let competing = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let busy = flow.code_callback(&flow.owner, &second).await;
        let other = flow
            .callback(Some(&bob), &format!("code={CODE}&state={}", bobs.state))
            .await;
        (busy, other)
    };
    let (stalled, (busy, other)) = tokio::join!(stalled, competing);
    assert_eq!(stalled.status, StatusCode::OK);
    assert_eq!(busy.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(busy.body.contains("busy"));
    assert_safe(&busy, &second);
    assert_eq!(other.status, StatusCode::OK);
    assert_eq!(flow.state_of(&flow.owner, &second).await, "pending");
    assert_eq!(flow.token.requests().len(), 2);
    flow.token.delay(Duration::ZERO);
    assert_eq!(
        flow.code_callback(&flow.owner, &second).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn token_endpoints_resolving_or_pointing_to_private_addresses_are_never_contacted() {
    hito_testkit::logs::anchor_callsite_interest();
    let logs = Capture::default();
    let captured = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || captured.clone())
        .finish();
    async {
        for endpoint in [
            "https://10.0.0.5:8443/token",
            "https://192.168.1.10/token",
            "https://172.16.0.1/token",
            "https://169.254.169.254/token",
            "https://100.64.0.1/token",
            "https://0.0.0.0/token",
            "https://224.0.0.1/token",
            "https://[fd00::1]/token",
            "https://[fe80::1]/token",
            "https://[::]/token",
            "https://[ff02::1]/token",
            "https://[::ffff:10.0.0.1]/token",
            "https://localhost/token",
        ] {
            let flow = Flow::start_with(Some(endpoint)).await;
            let attempt = flow.begin().await;
            let before = logs.0.lock().unwrap().len();
            let page = flow.code_callback(&flow.owner, &attempt).await;
            assert_eq!(page.status, StatusCode::BAD_GATEWAY, "{endpoint}");
            assert_safe(&page, &attempt);
            assert_eq!(
                flow.state_of(&flow.owner, &attempt).await,
                "failed",
                "{endpoint}"
            );
            assert!(flow.token.requests().is_empty());
            let output = String::from_utf8(logs.0.lock().unwrap()[before..].to_vec()).unwrap();
            assert!(
                output.contains("\"reason\":\"blocked_address\""),
                "{endpoint}: {output}"
            );
        }
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn expiry_during_the_exchange_closes_the_attempt_instead_of_completing_it() {
    let flow = Flow::start().await;
    flow.token.delay(Duration::from_millis(600));
    let attempt = flow.begin().await;
    let aging = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        flow.sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1").await;
    };
    let (page, ()) = tokio::join!(flow.code_callback(&flow.owner, &attempt), aging);
    assert_eq!(page.status, StatusCode::CONFLICT);
    assert_safe(&page, &attempt);
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(flow.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let state: String =
        sqlx::query_scalar("SELECT status FROM plugin_authorization_attempts WHERE id = ?")
            .bind(&attempt.id)
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(state, "failed");
}

#[tokio::test]
async fn numeric_string_lifetimes_and_mixed_case_identity_encoding_are_accepted() {
    for reply in [Reply::GrantStringExpiry, Reply::GrantUpperIdentity] {
        let flow = Flow::start().await;
        flow.token.reply(reply);
        let attempt = flow.begin().await;
        assert_eq!(
            flow.code_callback(&flow.owner, &attempt).await.status,
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn codes_up_to_4096_bytes_are_exchanged_and_longer_codes_are_refused_unconsumed() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let long = "c".repeat(4097);
    let page = flow
        .callback(
            Some(&flow.owner),
            &format!("code={long}&state={}", attempt.state),
        )
        .await;
    assert_eq!(page.status, StatusCode::BAD_REQUEST);
    assert!(flow.token.requests().is_empty());
    let exact = "c".repeat(4096);
    let page = flow
        .callback(
            Some(&flow.owner),
            &format!("code={exact}&state={}", attempt.state),
        )
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(flow.token.requests()[0]["code"], exact);
}

#[tokio::test]
async fn a_locked_session_gets_an_html_page_and_leaves_the_attempt_unused() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let (status, _) = call(
        &flow.app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password": "administrator test password", "pin": "2468", "idle_minutes": 5}),
        Some(&flow.owner),
    )
    .await;
    assert!(status.is_success());
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&flow.owner),
    )
    .await;
    assert!(status.is_success());
    let page = flow.code_callback(&flow.owner, &attempt).await;
    assert_eq!(page.status, StatusCode::LOCKED);
    assert!(page.body.starts_with("<!doctype html>"));
    assert!(page.body.contains("locked"));
    assert_safe(&page, &attempt);
    assert!(flow.token.requests().is_empty());
}

#[tokio::test]
async fn the_cross_site_retry_is_bounded_by_a_marker_and_pages_refuse_referrers() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let query = format!("code={CODE}&state={}", attempt.state);
    let first = flow
        .callback_with(None, &query, &[("sec-fetch-site", "cross-site")])
        .await;
    assert!(first.body.contains("http-equiv=\"refresh\" content=\"1\""));
    let marker = first.headers["set-cookie"].to_str().unwrap().to_owned();
    assert!(marker.starts_with("hito_callback_retry=1;"));
    assert!(
        marker.contains("Max-Age=30")
            && marker.contains("HttpOnly")
            && marker.contains("SameSite=Lax")
    );
    assert!(marker.contains("Path=/api/v1/plugin-authorization/callback"));
    assert_safe(&first, &attempt);
    let again = flow
        .callback_with(
            None,
            &query,
            &[
                ("sec-fetch-site", "cross-site"),
                ("cookie", "hito_callback_retry=1"),
            ],
        )
        .await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
    assert!(!again.body.contains("http-equiv"));
    assert_safe(&again, &attempt);
}

#[tokio::test]
async fn a_session_that_arrives_with_the_retry_marker_clears_it_so_later_callbacks_can_retry() {
    let flow = Flow::start().await;
    let attempt = flow.begin().await;
    let cookie = format!("hito_session={}; hito_callback_retry=1", flow.owner);
    let query = format!("error=access_denied&state={}", attempt.state);
    let page = flow
        .callback_with(None, &query, &[("cookie", cookie.as_str())])
        .await;
    assert_eq!(page.status, StatusCode::OK);
    let cleared = page.headers["set-cookie"].to_str().unwrap();
    assert!(cleared.starts_with("hito_callback_retry=;") && cleared.contains("Max-Age=0"));
    assert!(cleared.contains("Path=/api/v1/plugin-authorization/callback"));
    let plain = flow.callback(Some(&flow.owner), "state=missing").await;
    assert!(plain.headers.get("set-cookie").is_none());
}
