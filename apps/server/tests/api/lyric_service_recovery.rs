use super::support;
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_plugin_sdk::{LYRICS_V2_CAPABILITIES_PATH, LYRICS_V2_PATH, METADATA_PATH, SEARCH_PATH};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};

use crate::account;

#[path = "../fixtures/lyric_credential_recovery.rs"]
mod fixture;

use fixture::{
    assert_document_owner, assert_search_owner, document_recovery_metadata,
    document_recovery_reference, document_recovery_response,
};

#[tokio::test]
async fn a_new_successful_protected_request_clears_only_its_owners_prior_credential_failure() {
    let mut plugin = fixture::protected_plugin(
        "org.hito.credential-recovery-fixture",
        "Owned credential recovery service",
    )
    .post(SEARCH_PATH, |request| {
        assert_search_owner(request);
        Reply::Status(StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    let result = AssertUnwindSafe(exercise(&plugin)).catch_unwind().await;
    assert!(plugin.finish().await);
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation["health"]["state"], "ok",
        "A new checked protected success must clear its owner's older credential rejection"
    );
}

async fn exercise(plugin: &FakePlugin) -> Value {
    let setup = fixture::SearchSetup::prepare(plugin).await;
    let library = &setup.library;
    let installation = &setup.installation;
    assert_eq!(plugin.health_requests().len(), 2);
    let path = format!("/api/v1/recordings/{}/lyrics/candidates", library.recording);
    let request = json!({
        "installation_id":installation["id"],
        "installation_revision":installation["revision"],
        "query":"Owned credential recovery query",
    });
    let (status, unauthenticated) =
        support::call(&library.app, "POST", &path, request.clone(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(unauthenticated.get("plugin_connection").is_none());
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let (status, headers, response) = account::exchange(
        &library.app,
        "POST",
        &path,
        request.clone(),
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response["code"], "plugin_credentials_required");
    assert_eq!(
        response["plugin_connection"],
        json!({
            "installation_id":installation["id"],"installation_revision":installation["revision"],
        })
    );
    let rejected_at = Instant::now();
    let rejected = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(
        rejected["installations"][0]["health"]["state"],
        "credentials_required"
    );
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.health_requests().len(), 2);
    plugin.on(SEARCH_PATH, |request| {
        assert_search_owner(request);
        Reply::Json(serde_json::to_value(fixture::empty_search_page()).unwrap())
    });
    let (status, headers, response) = account::exchange(
        &library.app,
        "POST",
        &path,
        request,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response, json!({"items":[],"next_cursor":null}));
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 2);
    let recovered = fixture::private_settings(library).await;
    let current = setup.assert_preserved(&recovered).await;
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
    assert!(rejected_at.elapsed() < Duration::from_secs(20));
    current
}

#[tokio::test]
async fn a_checked_selected_v2_document_success_clears_its_owners_prior_lyric_rejection() {
    let mut plugin = fixture::reviewed_plugin(
        "org.hito.document-recovery-fixture",
        "Owned document recovery service",
    )
    .get(LYRICS_V2_CAPABILITIES_PATH, |_| {
        Reply::Json(
            serde_json::to_value(hito_plugin_sdk::LyricsCapabilitiesV2 { lyrics_version: 2 })
                .unwrap(),
        )
    })
    .post(LYRICS_V2_PATH, |request| {
        assert_document_owner(request);
        assert_eq!(
            request.json(),
            json!({"recording":document_recovery_reference()})
        );
        Reply::Status(StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    let result = AssertUnwindSafe(exercise_document_recovery(&plugin))
        .catch_unwind()
        .await;
    assert!(plugin.finish().await);
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation["health"]["state"], "ok",
        "A validated selected lyric document must clear its owner's older credential rejection"
    );
}

async fn exercise_document_recovery(plugin: &FakePlugin) -> Value {
    use sha2::{Digest, Sha256};
    let fixture::ReviewedSetup {
        library,
        installation,
        base,
        candidate,
    } = fixture::ReviewedSetup::prepare(plugin).await;
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        review["candidate"],
        serde_json::to_value(document_recovery_metadata()).unwrap()
    );
    let (status, identity) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity["selector"]["kind"], "reviewed");
    let selector: hito_core::RecordingLyricSelector =
        serde_json::from_value(identity["selector"].clone()).unwrap();
    let state = fixture::ReviewedState::prepare(&library, &installation).await;
    let baseline = state.capture(&library, &base, &identity).await;
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 2);
    let path = format!("{base}/identities/read");
    let request = json!({"selector":selector,"installation_revision":installation["revision"]});
    let (status, failure_headers, failure) = account::exchange(
        &library.app,
        "POST",
        &path,
        request.clone(),
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(failure_headers["cache-control"], "private, no-store");
    assert_eq!(failure["code"], "plugin_credentials_required");
    assert_eq!(
        failure["plugin_connection"],
        json!({
            "installation_id":installation["id"],"installation_revision":installation["revision"],
        })
    );
    assert_eq!(failure.as_object().unwrap().len(), 3);
    assert!(
        failure["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty() && text.len() <= 240)
    );
    let rejected_at = Instant::now();
    let diagnosed = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(
        diagnosed["installations"][0]["health"]["state"],
        "credentials_required"
    );
    plugin.on(LYRICS_V2_PATH, |request| {
        assert_document_owner(request);
        assert_eq!(
            request.json(),
            json!({"recording":document_recovery_reference()})
        );
        Reply::Json(serde_json::to_value(document_recovery_response()).unwrap())
    });
    let (status, headers, response) = account::exchange(
        &library.app,
        "POST",
        &path,
        request,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let document = document_recovery_response();
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&document).unwrap())
    );
    let expected = hito_core::SelectedLyricsLookup::ReadyV2 {
        recording_id: library.recording.clone(),
        selector,
        provider_recording: document_recovery_reference(),
        document_digest: digest,
        document: Box::new(document),
    };
    assert_eq!(response, serde_json::to_value(expected).unwrap());
    let recovered = fixture::private_settings(&library).await;
    let current = state
        .assert_preserved(&library, &installation, &base, &baseline, &recovered)
        .await;
    assert_eq!(plugin.requests_for(LYRICS_V2_PATH).len(), 2);
    assert_eq!(plugin.requests_for(LYRICS_V2_CAPABILITIES_PATH).len(), 2);
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 2);
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
    assert!(rejected_at.elapsed() < Duration::from_secs(20));
    current
}

#[tokio::test]
async fn a_checked_lyric_metadata_review_success_clears_its_owners_prior_credential_rejection() {
    let mut plugin = fixture::reviewed_plugin(
        "org.hito.metadata-recovery-fixture",
        "Owned metadata recovery service",
    )
    .start()
    .await;
    let result = AssertUnwindSafe(exercise_metadata_recovery(&plugin))
        .catch_unwind()
        .await;
    assert!(plugin.finish().await);
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation["health"]["state"], "ok",
        "A validated lyric metadata review must clear its owner's older credential rejection"
    );
}

async fn exercise_metadata_recovery(plugin: &FakePlugin) -> Value {
    let fixture::ReviewedSetup {
        library,
        installation,
        base,
        candidate,
    } = fixture::ReviewedSetup::prepare(plugin).await;
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        review["candidate"],
        serde_json::to_value(document_recovery_metadata()).unwrap()
    );
    let (status, identity) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity["selector"]["kind"], "reviewed");
    let state = fixture::ReviewedState::prepare(&library, &installation).await;
    let (status, current_review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        current_review["candidate"],
        serde_json::to_value(document_recovery_metadata()).unwrap()
    );
    assert!(
        current_review["review_stamp"]
            .as_str()
            .is_some_and(|stamp| stamp.len() == 82 && stamp.starts_with("lyric-identity-v1:"))
    );
    assert_ne!(
        current_review["review_stamp"], review["review_stamp"],
        "The review must bind the current personal title rather than the earlier target"
    );
    let baseline = state.capture(&library, &base, &identity).await;
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 3);
    plugin.on(METADATA_PATH, |request| {
        assert_document_owner(request);
        assert_eq!(
            request.json(),
            json!({"recording_id":"owned:document-recovery"})
        );
        Reply::Status(StatusCode::UNAUTHORIZED)
    });
    let path = format!("{base}/identities/review");
    let request = candidate;
    let (status, failure_headers, failure) = account::exchange(
        &library.app,
        "POST",
        &path,
        request.clone(),
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(failure_headers["cache-control"], "private, no-store");
    assert_eq!(failure["code"], "plugin_credentials_required");
    assert_eq!(
        failure["plugin_connection"],
        json!({
            "installation_id":installation["id"],"installation_revision":installation["revision"],
        })
    );
    assert_eq!(failure.as_object().unwrap().len(), 3);
    assert!(
        failure["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty() && text.len() <= 240)
    );
    let rejected_at = Instant::now();
    let diagnosed = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(
        diagnosed["installations"][0]["health"]["state"],
        "credentials_required"
    );
    plugin.on(METADATA_PATH, |request| {
        assert_document_owner(request);
        assert_eq!(
            request.json(),
            json!({"recording_id":"owned:document-recovery"})
        );
        Reply::Json(serde_json::to_value(document_recovery_metadata()).unwrap())
    });
    let (status, headers, response) = account::exchange(
        &library.app,
        "POST",
        &path,
        request,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response, current_review);
    let recovered = fixture::private_settings(&library).await;
    let current = state
        .assert_preserved(&library, &installation, &base, &baseline, &recovered)
        .await;
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 5);
    assert!(plugin.requests_for(LYRICS_V2_PATH).is_empty());
    assert!(plugin.requests_for(LYRICS_V2_CAPABILITIES_PATH).is_empty());
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.unexpected(), 0);
    assert!(rejected_at.elapsed() < Duration::from_secs(20));
    current
}

#[path = "lyric_automatic_snapshot.rs"]
mod automatic_snapshot;

#[path = "lyric_identity_snapshot.rs"]
mod identity_snapshot;

#[path = "lyric_selected_snapshot.rs"]
mod selected_snapshot;

#[path = "lyric_automatic_target_snapshot.rs"]
mod automatic_target_snapshot;

#[path = "lyric_identity_target_snapshot.rs"]
mod identity_target_snapshot;

#[path = "lyric_selected_target_snapshot.rs"]
mod selected_target_snapshot;

#[path = "lyric_ticket_dispatch_order.rs"]
mod ticket_dispatch_order;
