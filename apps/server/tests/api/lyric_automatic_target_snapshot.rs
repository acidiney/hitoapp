use super::fixture;
use crate::account;
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    LYRICS_V2_CAPABILITIES_PATH, LYRICS_V2_PATH, MANIFEST_PATH, METADATA_PATH, SEARCH_PATH,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use hito_testkit::held_authority_snapshot::HeldSnapshot;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

type Response = (StatusCode, HeaderMap, Value);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn title_edit_committed_during_automatic_search_postcheck_preserves_private_credential_health()
 {
    let mut plugin = fixture::reviewed_plugin(
        "org.hito.automatic-title-snapshot",
        "Owned automatic title snapshot lyrics",
    )
    .post(SEARCH_PATH, |request| {
        fixture::assert_document_owner(request);
        assert_eq!(
            request.json(),
            json!({
                "query":"Owned automatic snapshot performer Owned document recovery title pin",
                "cursor":null,"limit":20,
            })
        );
        Reply::Status(StatusCode::UNAUTHORIZED)
    })
    .start()
    .await;
    let hold = HeldSnapshot::within_span("automatic_lyric_authority", "lyric_search_postcheck");
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
    assert!(drained.is_ok(), "All owned HTTP tasks must drain");
    assert!(stopped, "The owned protocol provider must join");
    let (current, expected) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned automatic snapshot workflow must settle within its deadline"),
    };
    assert_eq!(
        current, expected,
        "An automatic protected401 checked through a snapshot older than committed personal title edit must not publish private credential health"
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
    let (status, receipt) = library.account.request(&library.app, "POST", "/api/v1/sync", json!({
        "device_id":"owned-automatic-snapshot-client","operations":[{
            "id":Uuid::new_v4(),"action":{"kind":"recording_artist_set",
                "recording_id":library.recording,"artist":"Owned automatic snapshot performer"},
        }],
    })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 1);
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
    let state = fixture::ReviewedState::prepare(library, &setup.installation).await;
    let baseline = state.capture(library, &setup.base, &identity).await;
    let before = fixture::private_settings(library).await;
    let expected = before["installations"][0].clone();
    assert_eq!(expected["id"], setup.installation["id"]);
    assert_eq!(expected["health"]["state"], "ok");
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 2);
    assert!(plugin.requests_for(SEARCH_PATH).is_empty());
    let manifests = plugin.requests_for(MANIFEST_PATH).len();
    let health = plugin.health_requests().len();
    let app = library.app.clone();
    let path = format!("{}/automatic", setup.base);
    let headers = library.account.headers();
    hito_testkit::logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::registry().with(hold.clone());
    requests.spawn(
        async move { account::exchange(&app, "POST", &path, json!({}), headers).await }
            .with_subscriber(subscriber),
    );
    hold.entered().await;
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert!(requests.try_join_next().is_none());
    let held_at = Instant::now();
    let operation = Uuid::new_v4();
    let device = "owned-automatic-title-edit-client";
    let new_title = "Owned title committed during protected search postcheck";
    let (status, receipt) = tokio::time::timeout(
        Duration::from_secs(2),
        library.account.request(
            &library.app,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id":device,"operations":[{
                    "id":operation,"action":{"kind":"recording_title_set",
                        "recording_id":library.recording,"title":new_title},
                }],
            }),
        ),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt, json!({"acknowledged":[operation]}));
    hold.assert_held();
    let (status, headers, metadata) = account::exchange(
        &library.app,
        "GET",
        &format!("/api/v1/recordings/{}/metadata", library.recording),
        Value::Null,
        inspector.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(metadata["recording_id"], library.recording);
    assert_eq!(metadata["title"]["value"], new_title);
    assert_eq!(metadata["title"]["personal"], new_title);
    assert_eq!(metadata["title"]["operation_id"], operation.to_string());
    let history = inspector.get(&library.app, "/api/v1/sync?cursor=0").await;
    let adjusted_baseline = baseline.after_public_title_edit(
        &library.recording,
        operation,
        device,
        new_title,
        &receipt,
        &history,
    );
    let (status, headers, during) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(during, before);
    hold.assert_held();
    assert!(requests.try_join_next().is_none());
    hold.release();
    let response = tokio::time::timeout(Duration::from_secs(2), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    hold.assert_returned();
    assert_eq!(response.0, StatusCode::CONFLICT);
    assert_eq!(response.1["cache-control"], "private, no-store");
    assert!(response.2.get("plugin_connection").is_none());
    assert!(response.2.get("code").is_none());
    assert!(response.2.get("document").is_none());
    assert!(response.2.get("candidate").is_none());
    assert!(response.2.get("review_stamp").is_none());
    let encoded = response.2.to_string();
    for private in [
        fixture::TOKEN,
        "owned-document-peer-token",
        "owned:document-recovery",
        "Owned reviewed document title",
        "Owned reviewed document artist",
        plugin.url.as_str(),
        new_title,
        "Owned document recovery title pin",
        "Owned automatic snapshot performer",
    ] {
        assert!(!encoded.contains(private));
    }
    let after = fixture::private_settings(library).await;
    let current = state
        .assert_preserved(
            library,
            &setup.installation,
            &setup.base,
            &adjusted_baseline,
            &after,
        )
        .await;
    assert_eq!(plugin.requests_for(SEARCH_PATH).len(), 1);
    assert_eq!(plugin.requests_for(METADATA_PATH).len(), 2);
    assert_eq!(plugin.requests_for(MANIFEST_PATH).len(), manifests);
    assert_eq!(plugin.health_requests().len(), health);
    for path in [LYRICS_V2_CAPABILITIES_PATH, LYRICS_V2_PATH] {
        assert!(plugin.requests_for(path).is_empty());
    }
    assert_eq!(plugin.unexpected(), 0);
    assert!(held_at.elapsed() < Duration::from_secs(20));
    (current, expected)
}
