use super::authorization_flow::*;
use super::support::{browser_call, call};
use axum::http::StatusCode;
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tracing::instrument::WithSubscriber;

const VERIFIER: &str = "owned-app-verifier-0123456789-abcdefghijklmnopqrstuvwxyz";

fn hash(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

struct AppReturn {
    location: String,
    attempt: String,
    secret: String,
}

fn app_return(page: &Page) -> AppReturn {
    assert_eq!(page.status, StatusCode::SEE_OTHER, "{}", page.body);
    assert_eq!(page.headers["cache-control"], "no-store");
    assert_eq!(page.headers["referrer-policy"], "no-referrer");
    let location = page.headers["location"].to_str().unwrap().to_owned();
    let url: url::Url = location.parse().unwrap();
    assert_eq!(url.scheme(), "hito");
    assert_eq!(url.host_str(), Some("plugin-authorization"));
    assert_eq!(url.path(), "/return");
    assert!(url.fragment().is_none());
    let pairs: BTreeMap<String, String> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(
        pairs.keys().map(String::as_str).collect::<Vec<_>>(),
        ["attempt", "return"]
    );
    AppReturn {
        attempt: pairs["attempt"].clone(),
        secret: pairs["return"].clone(),
        location,
    }
}

async fn provider_returns(flow: &Flow, attempt: &Attempt, browser: Option<&str>) -> Page {
    flow.callback(browser, &format!("code={CODE}&state={}", attempt.state))
        .await
}

async fn member(flow: &Flow) -> String {
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/users",
        json!({"username": "bob", "password": "bob native test password"}),
        Some(&flow.owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    flow.login("bob", "bob native test password").await
}

#[tokio::test]
async fn the_app_signs_in_through_the_app_return_and_finishes_with_both_secrets() {
    let flow = Flow::start().await;
    let attempt = flow.begin_native(&flow.owner, VERIFIER).await;
    let page = provider_returns(&flow, &attempt, None).await;
    let back = app_return(&page);
    assert_eq!(back.attempt, attempt.id);
    for private in [
        CODE,
        attempt.state.as_str(),
        attempt.challenge.as_str(),
        VERIFIER,
        hash(VERIFIER).as_str(),
    ] {
        assert!(!back.location.contains(private));
        assert!(!page.body.contains(private));
    }
    assert!(flow.token.requests().is_empty());
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "returned");

    let (status, body) = flow
        .complete_native(
            &flow.owner,
            &attempt,
            json!({"verifier": VERIFIER, "return_secret": back.secret}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"outcome": "connected"}));
    let requests = flow.token.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["code"], CODE);
    assert_eq!(
        requests[0]["redirect_uri"],
        "https://hito.test/api/v1/plugin-authorization/callback"
    );
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "completed");
    assert_eq!(flow.connection(&flow.owner).await.1["status"], "connected");

    let (status, body) = flow
        .complete_native(
            &flow.owner,
            &attempt,
            json!({"verifier": VERIFIER, "return_secret": back.secret}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"outcome": "used"}));
    let replay = provider_returns(&flow, &attempt, None).await;
    assert_eq!(replay.status, StatusCode::CONFLICT);
    assert!(replay.headers.get("location").is_none());
    assert_eq!(flow.token.requests().len(), 1);
}

#[tokio::test]
async fn a_member_cannot_finish_a_sign_in_that_another_member_approved() {
    let flow = Flow::start().await;
    let bob = member(&flow).await;
    let attempt = flow.begin_native(&flow.owner, VERIFIER).await;
    for browser in [Some(bob.as_str()), None] {
        let page = provider_returns(&flow, &attempt, browser).await;
        if browser.is_some() {
            let landed = app_return(&page);
            assert_eq!(landed.attempt, attempt.id);
            let (status, _) = flow
                .complete_native(
                    &bob,
                    &attempt,
                    json!({"verifier": VERIFIER, "return_secret": landed.secret}),
                )
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        } else {
            assert_eq!(page.status, StatusCode::CONFLICT);
        }
    }
    let (status, _) = flow
        .complete_native(&flow.owner, &attempt, json!({"verifier": VERIFIER}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "returned");
    let (status, body) = flow
        .complete_native(
            &flow.owner,
            &attempt,
            json!({"verifier": VERIFIER, "return_secret": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"outcome": "invalid"}));
    assert_eq!(flow.state_of(&flow.owner, &attempt).await, "failed");
    assert!(flow.token.requests().is_empty());
    assert_eq!(
        flow.connection(&flow.owner).await.1["status"],
        "not_connected"
    );
}

#[tokio::test]
async fn bearer_starts_need_a_verifier_hash_and_browser_starts_cannot_send_one() {
    let flow = Flow::start().await;
    let revision = flow.installation["revision"].clone();
    for body in [
        json!({"revision": revision}),
        json!({"revision": revision, "verifier_hash": "short"}),
        json!({"revision": revision, "verifier_hash": format!("{}=", &hash(VERIFIER)[..42])}),
    ] {
        let (status, refused) = call(
            &flow.app,
            "POST",
            &flow.begin_path(),
            body.clone(),
            Some(&flow.owner),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body} {refused}");
    }
    let (status, _) = browser_call(
        &flow.app,
        "POST",
        &flow.begin_path(),
        json!({"revision": revision, "verifier_hash": hash(VERIFIER)}),
        &flow.owner,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let browser = flow.begin().await;
    let page = flow.code_callback(&flow.owner, &browser).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert!(page.body.contains("Provider authorized"));
}

#[tokio::test]
async fn completion_needs_the_starting_session_unlocked_and_still_signed_in() {
    let flow = Flow::start().await;
    let attempt = flow.begin_native(&flow.owner, VERIFIER).await;
    let back = app_return(&provider_returns(&flow, &attempt, None).await);
    let body = json!({"verifier": VERIFIER, "return_secret": back.secret});
    let other_session = flow.login("admin", "administrator test password").await;
    assert_eq!(
        flow.complete_native(&other_session, &attempt, body.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
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
    assert_eq!(
        flow.complete_native(&flow.owner, &attempt, body.clone())
            .await
            .0,
        StatusCode::LOCKED
    );
    let (status, _) = call(
        &flow.app,
        "POST",
        "/api/v1/logout",
        Value::Null,
        Some(&flow.owner),
    )
    .await;
    assert!(status.is_success());
    assert_eq!(
        flow.complete_native(&flow.owner, &attempt, body).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(flow.token.requests().is_empty());
}

#[tokio::test]
async fn an_expired_native_attempt_is_refused_without_an_exchange() {
    let flow = Flow::start().await;
    let attempt = flow.begin_native(&flow.owner, VERIFIER).await;
    let back = app_return(&provider_returns(&flow, &attempt, None).await);
    flow.sql("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1").await;
    let (status, body) = flow
        .complete_native(
            &flow.owner,
            &attempt,
            json!({"verifier": VERIFIER, "return_secret": back.secret}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"outcome": "expired"}));
    assert!(flow.token.requests().is_empty());
}

#[tokio::test]
async fn native_sign_in_logs_carry_no_code_state_verifier_or_return_secret() {
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
        let good = flow.begin_native(&flow.owner, VERIFIER).await;
        let back = app_return(&provider_returns(&flow, &good, None).await);
        let (status, _) = flow
            .complete_native(
                &flow.owner,
                &good,
                json!({"verifier": VERIFIER, "return_secret": back.secret}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let bad = flow.begin_native(&flow.owner, VERIFIER).await;
        let wrong = app_return(&provider_returns(&flow, &bad, None).await);
        let (status, _) = flow
            .complete_native(
                &flow.owner,
                &bad,
                json!({"verifier": "owned-wrong-app-verifier-0123456789abcdefghij", "return_secret": wrong.secret}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let verifier = flow.token.requests()[0]["code_verifier"].clone();
        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        let entries: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for message in [
            "Provider authorization returned to the app",
            "Provider authorization code exchanged",
            "Provider authorization completion rejected or closed",
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
            good.state.as_str(),
            good.challenge.as_str(),
            bad.state.as_str(),
            verifier.as_str(),
            VERIFIER,
            hash(VERIFIER).as_str(),
            back.secret.as_str(),
            wrong.secret.as_str(),
            "hito://",
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
