#[path = "../fixtures/source_choice_accounts.rs"]
mod accounts;
#[allow(dead_code)]
#[path = "../fixtures/manual_choice_http.rs"]
mod choice_http;
#[allow(dead_code)]
#[path = "../fixtures/manual_source.rs"]
mod fixture;
#[path = "../fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../fixtures/artwork_http.rs"]
mod http;

use accounts::{SavedAccount, assert_public_ack, assert_reset};
use axum::http::StatusCode;
use choice_http::{assert_choice, call};
use fixture::Provider;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn source_choice_receipts_are_account_scoped_and_revoked_sessions_cannot_replay_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut provider = Provider::start("authority", 0).await;
    let mut hosts = Vec::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(async {
            hosts.push(
                host::Host::start(host::open(directory.path(), "source-authority-host").await)
                    .await,
            );
            exercise(hosts[0].address, &provider).await;
        })
        .catch_unwind(),
    )
    .await;
    for server in &mut hosts {
        server.request_shutdown();
    }
    let mut joined = true;
    for server in &mut hosts {
        joined &= server.join().await;
    }
    joined &= provider.finish().await;
    assert!(
        joined,
        "all real hosts and the owned provider must join on success, panic or timeout"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned account and session lifecycle must finish within its deadline"),
    }
}

async fn exercise(address: SocketAddr, provider: &Provider) {
    let alice_credentials =
        json!({"username": "alice", "password": "owned Alice authority password"});
    let alice_session = call(
        address,
        "POST",
        "/api/v1/setup",
        alice_credentials.clone(),
        None,
        StatusCode::CREATED,
    )
    .await;
    assert!(
        alice_session["user"]["admin"] == true,
        "Alice must be the administrator who offers the service"
    );
    let service = call(
        address,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned account authority service", "base_url": provider.url}),
        alice_session["token"].as_str(),
        StatusCode::CREATED,
    )
    .await;
    let alice = SavedAccount::adopt(address, &alice_session, &service, provider).await;
    let request_x = Uuid::new_v4();
    let alice_choose = json!({"request_id": request_x, "source_id": alice.source});
    let alice_setup_calls = provider.requests();
    let alice_receipt = call(
        address,
        "POST",
        &alice.choice_path,
        alice_choose.clone(),
        Some(&alice.token),
        StatusCode::OK,
    )
    .await;
    assert_choice(&alice_receipt, &alice.recording, &alice_choose);
    assert!(
        alice_receipt["revision"] == 1,
        "Alice's first selection must have its own revision"
    );
    assert_public_ack(&alice_receipt, provider);
    let alice_first = alice.snapshot(address, &alice.token, "manual", 1).await;
    assert_eq!(
        provider.requests(),
        alice_setup_calls,
        "Alice's first choice and snapshot must not call the provider"
    );

    let bob_credentials = json!({"username": "bob", "password": "owned Bob authority password"});
    let created_bob = call(
        address,
        "POST",
        "/api/v1/users",
        bob_credentials.clone(),
        Some(&alice.token),
        StatusCode::CREATED,
    )
    .await;
    assert!(
        created_bob["admin"] == false && created_bob["id"] != alice_session["user"]["id"],
        "Bob must be a separate ordinary account"
    );
    let bob_session = call(
        address,
        "POST",
        "/api/v1/login",
        bob_credentials,
        None,
        StatusCode::OK,
    )
    .await;
    assert!(
        bob_session["user"]["admin"] == false && bob_session["user"]["id"] == created_bob["id"],
        "the second session must authenticate ordinary Bob"
    );
    let bob = SavedAccount::adopt(address, &bob_session, &service, provider).await;
    assert!(
        alice.recording != bob.recording && alice.source != bob.source,
        "the same offered provider must yield independent private Hito recording and source identities"
    );
    let control_calls = provider.requests();
    assert!(
        control_calls > alice_setup_calls
            && provider.audio_requests() == 0
            && provider.unexpected() == 0,
        "both accounts must finish valid provider setup without fetching audio"
    );
    let bob_initial = bob.snapshot(address, &bob.token, "automatic", 0).await;

    for (method, path, body) in [
        ("GET", alice.choice_path.as_str(), None),
        (
            "POST",
            alice.choice_path.as_str(),
            Some(json!({"request_id": Uuid::new_v4(), "source_id": alice.source})),
        ),
        (
            "POST",
            alice.choice_path.as_str(),
            Some(alice_choose.clone()),
        ),
        (
            "POST",
            alice.reset_path.as_str(),
            Some(json!({"request_id": Uuid::new_v4(), "expected_revision": 1})),
        ),
    ] {
        let denied = http::call(address, method, path, body, Some(&bob.token)).await;
        assert_eq!(
            denied.status,
            StatusCode::NOT_FOUND,
            "Bob must not read, mutate or replay controls for Alice's private recording"
        );
    }
    call(
        address,
        "POST",
        &bob.choice_path,
        json!({"request_id": request_x, "source_id": alice.source}),
        Some(&bob.token),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert!(
        alice.snapshot(address, &alice.token, "manual", 1).await == alice_first,
        "foreign requests must leave Alice's complete choice unchanged"
    );
    assert!(
        bob.snapshot(address, &bob.token, "automatic", 0).await == bob_initial,
        "a foreign source selector must not alter Bob's own choice or consume its request identity"
    );

    let bob_choose = json!({"request_id": request_x, "source_id": bob.source});
    let bob_receipt = call(
        address,
        "POST",
        &bob.choice_path,
        bob_choose.clone(),
        Some(&bob.token),
        StatusCode::OK,
    )
    .await;
    assert_choice(&bob_receipt, &bob.recording, &bob_choose);
    assert!(
        bob_receipt["request_id"] == alice_receipt["request_id"]
            && bob_receipt["revision"] == 1
            && bob_receipt["recording_id"] != alice_receipt["recording_id"]
            && bob_receipt["source_id"] != alice_receipt["source_id"],
        "the same command UUID must yield separate account-owned acknowledgements without a cross-account conflict"
    );
    assert_public_ack(&bob_receipt, provider);
    bob.snapshot(address, &bob.token, "manual", 1).await;
    assert!(
        alice.snapshot(address, &alice.token, "manual", 1).await == alice_first,
        "Bob's independent use of the UUID must not mutate Alice's receipt or pin"
    );

    let request_y = Uuid::new_v4();
    let reset = json!({"request_id": request_y, "expected_revision": 1});
    let alice_reset = call(
        address,
        "POST",
        &alice.reset_path,
        reset.clone(),
        Some(&alice.token),
        StatusCode::OK,
    )
    .await;
    let bob_reset = call(
        address,
        "POST",
        &bob.reset_path,
        reset.clone(),
        Some(&bob.token),
        StatusCode::OK,
    )
    .await;
    assert_reset(&alice_reset, request_y, &alice.recording, 2);
    assert_reset(&bob_reset, request_y, &bob.recording, 2);
    assert!(
        alice_reset["request_id"] == bob_reset["request_id"]
            && alice_reset["recording_id"] != bob_reset["recording_id"],
        "reset UUIDs must also have independent immutable account receipts"
    );
    assert_public_ack(&alice_reset, provider);
    assert_public_ack(&bob_reset, provider);
    let alice_automatic = alice.snapshot(address, &alice.token, "automatic", 2).await;
    let bob_automatic = bob.snapshot(address, &bob.token, "automatic", 2).await;
    for (path, body) in [
        (alice.choice_path.as_str(), alice_choose.clone()),
        (alice.reset_path.as_str(), reset.clone()),
    ] {
        call(
            address,
            "POST",
            path,
            body,
            Some(&bob.token),
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    assert!(
        alice.snapshot(address, &alice.token, "automatic", 2).await == alice_automatic,
        "even existing Bob receipts must not authorize replay against Alice's recording"
    );
    assert!(
        bob.snapshot(address, &bob.token, "automatic", 2).await == bob_automatic,
        "foreign receipt requests must not alter Bob's current automatic state"
    );

    let later_choice = json!({"request_id": Uuid::new_v4(), "source_id": alice.source});
    let current = call(
        address,
        "POST",
        &alice.choice_path,
        later_choice.clone(),
        Some(&alice.token),
        StatusCode::OK,
    )
    .await;
    assert_choice(&current, &alice.recording, &later_choice);
    assert!(
        current["revision"] == 3,
        "the current Alice pin must be newer than both original acknowledgements"
    );
    assert_public_ack(&current, provider);
    let alice_current = alice.snapshot(address, &alice.token, "manual", 3).await;
    assert_eq!(
        provider.requests(),
        control_calls,
        "all source reads, choices, resets, replays and denied requests must avoid provider IO"
    );

    call(
        address,
        "POST",
        "/api/v1/logout",
        Value::Null,
        Some(&alice.token),
        StatusCode::NO_CONTENT,
    )
    .await;
    for (method, path, body) in [
        ("GET", alice.choice_path.as_str(), None),
        (
            "POST",
            alice.choice_path.as_str(),
            Some(alice_choose.clone()),
        ),
        ("POST", alice.reset_path.as_str(), Some(reset.clone())),
    ] {
        let denied = http::call(address, method, path, body, Some(&alice.token)).await;
        assert_eq!(
            denied.status,
            StatusCode::UNAUTHORIZED,
            "a revoked bearer must not read controls or replay either stored acknowledgement"
        );
    }
    let resumed = call(
        address,
        "POST",
        "/api/v1/login",
        alice_credentials,
        None,
        StatusCode::OK,
    )
    .await;
    let fresh_token = resumed["token"].as_str().unwrap();
    assert!(
        fresh_token != alice.token && resumed["user"]["id"] == alice_session["user"]["id"],
        "Alice must resume with a fresh session for the same account"
    );
    assert!(
        alice.snapshot(address, fresh_token, "manual", 3).await == alice_current,
        "logout must not erase the stored current pin"
    );
    let alice_replay = call(
        address,
        "POST",
        &alice.choice_path,
        alice_choose,
        Some(fresh_token),
        StatusCode::OK,
    )
    .await;
    let reset_replay = call(
        address,
        "POST",
        &alice.reset_path,
        reset.clone(),
        Some(fresh_token),
        StatusCode::OK,
    )
    .await;
    assert!(
        alice_replay == alice_receipt && reset_replay == alice_reset,
        "a new authorized session must recover Alice's exact original immutable acknowledgements"
    );
    assert!(
        alice.snapshot(address, fresh_token, "manual", 3).await == alice_current,
        "recovering old acknowledgements must not reapply their obsolete revisions"
    );
    let bob_replay = call(
        address,
        "POST",
        &bob.choice_path,
        bob_choose,
        Some(&bob.token),
        StatusCode::OK,
    )
    .await;
    let bob_reset_replay = call(
        address,
        "POST",
        &bob.reset_path,
        reset,
        Some(&bob.token),
        StatusCode::OK,
    )
    .await;
    assert!(
        bob_replay == bob_receipt && bob_reset_replay == bob_reset,
        "Bob must recover only his independent original receipts for the same UUIDs"
    );
    assert!(
        bob.snapshot(address, &bob.token, "automatic", 2).await == bob_automatic,
        "Alice's logout and receipt recovery must leave Bob's account and current state unchanged"
    );
    for (account, token) in [(&alice, fresh_token), (&bob, bob.token.as_str())] {
        let response = http::call(
            address,
            "GET",
            &format!("/api/v1/recordings/{}/download", account.recording),
            None,
            Some(token),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::NOT_FOUND,
            "source controls must not create an implicit recording download"
        );
    }
    assert_eq!(
        provider.requests(),
        control_calls,
        "account switching, session revocation and receipt recovery must not contact the provider"
    );
    assert_eq!(
        provider.audio_requests(),
        0,
        "no control operation may fetch or play provider audio"
    );
    assert_eq!(provider.unexpected(), 0);
}
