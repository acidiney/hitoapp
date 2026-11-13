use super::fixture;
use crate::account;
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    AUDIO_PATH, DETAIL_PATH, LYRICS_CAPABILITIES_PATH, LYRICS_PATH, LYRICS_V2_CAPABILITIES_PATH,
    LYRICS_V2_PATH, MANIFEST_PATH, METADATA_PATH, SEARCH_PATH, SOURCES_PATH,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use hito_testkit::held_authority_snapshot::HeldSnapshot;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_committed_during_selected_document_postcheck_preserves_private_credential_health() {
    let mut plugin = fixture::reviewed_plugin(
        "org.hito.selected-lyric-snapshot",
        "Owned selected snapshot lyrics",
    )
    .get(LYRICS_V2_CAPABILITIES_PATH, |request| {
        assert_eq!(request.method.as_str(), "GET");
        fixture::assert_document_owner(request);
        Reply::Json(
            serde_json::to_value(hito_plugin_sdk::LyricsCapabilitiesV2 { lyrics_version: 2 })
                .unwrap(),
        )
    })
    .post(LYRICS_V2_PATH, |request| {
        assert_eq!(request.method.as_str(), "POST");
        fixture::assert_document_owner(request);
        assert_eq!(
            request.json(),
            json!({"recording":fixture::document_recovery_reference()})
        );
        Reply::Status(StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    let hold = HeldSnapshot::within_span("selected_lyric_authority", "lyric_document_postcheck");
    let mut requests = JoinSet::new();
    let mut setup = None;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            &plugin,
            &hold,
            &mut setup,
            &mut requests,
        )))
        .catch_unwind(),
    )
    .await;
    hold.release();
    requests.abort_all();
    let drained = tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await;
    let stopped = plugin.finish().await;
    if let Some(setup) = &setup {
        setup.library.assert_original();
    }
    assert!(
        drained.is_ok(),
        "All owned selected lyric HTTP tasks must drain"
    );
    assert!(
        stopped,
        "The owned selected lyric protocol provider must join"
    );
    let (current, expected) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The owned selected lyric snapshot workflow must settle within its deadline")
        }
    };
    assert_eq!(
        current, expected,
        "A selected document protected401 checked through a snapshot older than committed logout must not publish private credential health"
    );
}

async fn exercise(
    plugin: &FakePlugin,
    hold: &HeldSnapshot,
    setup_owner: &mut Option<fixture::ReviewedSetup>,
    requests: &mut JoinSet<Response>,
) -> (Value, Value) {
    let setup = setup_owner.insert(fixture::ReviewedSetup::prepare(plugin).await);
    let library = &mut setup.library;
    let inspector = account::Account::login(
        &library.app,
        "owner",
        "owned artist review administrator password",
    )
    .await;
    assert_eq!(inspector.id, library.account.id);
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{}/identities/review", setup.base),
            setup.candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        review["candidate"],
        serde_json::to_value(fixture::document_recovery_metadata()).unwrap()
    );
    let (status, identity) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{}/identities", setup.base),
            json!({"candidate":setup.candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity["selector"]["kind"], "reviewed");
    let selector: hito_core::RecordingLyricSelector =
        serde_json::from_value(identity["selector"].clone()).unwrap();
    let state = fixture::ReviewedState::prepare(library, &setup.installation).await;
    let baseline = state.capture(library, &setup.base, &identity).await;
    let before = fixture::private_settings(library).await;
    let expected = before["installations"][0].clone();
    assert_eq!(expected["id"], setup.installation["id"]);
    assert_eq!(expected["health"]["state"], "ok");
    let identities_path = format!("{}/identities", setup.base);
    let identities_before = library.account.get(&library.app, &identities_path).await;
    assert_eq!(identities_before["items"], json!([identity]));
    let ready_path = format!("/api/v1/recordings/{}/sources", library.recording);
    let ready_before = library.account.get(&library.app, &ready_path).await;
    assert!(
        ready_before
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["ready"] == true)
    );
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 2);
    assert_eq!(plugin.requests().len(), 4);
    assert!(plugin.requests_for(LYRICS_V2_CAPABILITIES_PATH).is_empty());
    assert!(plugin.requests_for(LYRICS_V2_PATH).is_empty());
    let app = library.app.clone();
    let path = format!("{}/identities/read", setup.base);
    let headers = library.account.headers();
    let request =
        json!({"selector":selector,"installation_revision":setup.installation["revision"]});
    hito_testkit::logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::registry().with(hold.clone());
    requests.spawn(
        async move { account::exchange(&app, "POST", &path, request, headers).await }
            .with_subscriber(subscriber),
    );
    hold.entered().await;
    assert_eq!(plugin.requests_for(LYRICS_V2_CAPABILITIES_PATH).len(), 1);
    assert_eq!(plugin.requests_for(LYRICS_V2_PATH).len(), 1);
    assert!(requests.try_join_next().is_none());
    let (status, _) = tokio::time::timeout(
        Duration::from_secs(2),
        library
            .account
            .request(&library.app, "POST", "/api/v1/logout", Value::Null),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    hold.assert_held();
    let (status, rejected_headers, rejected) = tokio::time::timeout(
        Duration::from_secs(2),
        account::exchange(
            &library.app,
            "GET",
            "/api/v1/plugins",
            Value::Null,
            library.account.headers(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(rejected_headers["cache-control"], "private, no-store");
    assert!(rejected.get("plugin_connection").is_none());
    assert!(rejected.get("code").is_none());
    hold.assert_held();
    let (status, during_headers, during) = tokio::time::timeout(
        Duration::from_secs(2),
        account::exchange(
            &library.app,
            "GET",
            "/api/v1/plugins",
            Value::Null,
            inspector.headers(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(during_headers["cache-control"], "private, no-store");
    assert_eq!(during, before);
    hold.assert_held();
    let identities_during = tokio::time::timeout(
        Duration::from_secs(2),
        inspector.get(&library.app, &identities_path),
    )
    .await
    .unwrap();
    assert_eq!(identities_during, identities_before);
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    assert_eq!(response.0, StatusCode::UNAUTHORIZED);
    assert_eq!(response.1["cache-control"], "private, no-store");
    for field in ["plugin_connection", "code", "document", "review_stamp"] {
        assert!(response.2.get(field).is_none());
    }
    let encoded = response.2.to_string();
    for private in [
        fixture::TOKEN,
        "owned-document-peer-token",
        "owned:document-recovery",
        "Owned reviewed document title",
        "Owned reviewed document artist",
        plugin.url.as_str(),
    ] {
        assert!(!encoded.contains(private));
    }
    library.account = inspector;
    assert_eq!(
        library.account.get(&library.app, &ready_path).await,
        ready_before
    );
    assert_eq!(
        library.account.get(&library.app, &identities_path).await,
        identities_before
    );
    let after = fixture::private_settings(library).await;
    let current = state
        .assert_preserved(library, &setup.installation, &setup.base, &baseline, &after)
        .await;
    let mut after_without_owner_health = after.clone();
    after_without_owner_health["installations"][0]["health"] = expected["health"].clone();
    assert_eq!(after_without_owner_health, before);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(plugin.health_requests().len(), 2);
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 2);
    assert_eq!(plugin.requests_for(LYRICS_V2_CAPABILITIES_PATH).len(), 1);
    assert_eq!(plugin.requests_for(LYRICS_V2_PATH).len(), 1);
    assert_eq!(plugin.requests().len(), 6);
    for path in [
        SEARCH_PATH,
        DETAIL_PATH,
        SOURCES_PATH,
        AUDIO_PATH,
        LYRICS_CAPABILITIES_PATH,
        LYRICS_PATH,
    ] {
        assert!(plugin.requests_for(path).is_empty());
    }
    assert_eq!(plugin.unexpected(), 0);
    (current, expected)
}
