use crate::{
    account::exchange,
    source_resolution_failure_fixture::{self as fixture, Case, Context, Library, Response},
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::{HANG_CEILING, hang_guard};
use hito_plugin_sdk::{
    AUDIO_PATH, DETAIL_PATH, MANIFEST_PATH, METADATA_PATH, SEARCH_PATH, SOURCES_PATH,
};
use hito_testkit::fake_plugin::{Handled, Reply};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn source_resolution_http_retains_typed_failure_and_replays_it_after_provider_recovery() {
    for case in [Case::Unavailable, Case::Refused] {
        let mut library = Library::new();
        let result = tokio::time::timeout(
            HANG_CEILING,
            AssertUnwindSafe(Box::pin(exercise(&mut library, case))).catch_unwind(),
        )
        .await;
        library.retire().await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(panic)) => resume_unwind(panic),
            Err(_) => {
                panic!("the owned HTTP disposition workflow must settle within its existing bound")
            }
        }
    }
}

async fn exercise(library: &mut Library, case: Case) {
    let context = hang_guard("owned HTTP disposition setup", library.prepare(case)).await;
    let provider = library.provider.as_ref().unwrap();
    let before = context.snapshot().await;
    let peer_before = context.peer_snapshot().await;
    let primary_before = library.primary_counts().await;
    assert_eq!(before["recordings"]["total"], 2);
    assert_eq!(before["metadata"]["title"]["value"], fixture::PRIVATE_TITLE);
    assert_eq!(
        before["metadata"]["selected_observation"]["title"],
        "Owned tune"
    );
    assert_eq!(before["progress"][0]["position_ms"], 400);
    assert_eq!(before["progress"][0]["speed"], 1.25);
    assert!(
        before["recordings"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == context.recording && item["liked"] == true)
    );
    let choice_path = format!("/api/v1/recordings/{}/source-choice", context.recording);
    let sources_path = format!("/api/v1/recordings/{}/sources", context.recording);
    let choice = context.owner.get(&context.app, &choice_path).await;
    assert_eq!(choice["mode"], "automatic");
    assert_eq!(
        context.owner.get(&context.app, &sources_path).await,
        json!([])
    );
    assert_eq!(provider.requests_for(MANIFEST_PATH).len(), 1);
    assert_eq!(provider.health_requests().len(), 1);
    assert!(provider.requests_for(SEARCH_PATH).is_empty());
    let request = json!({"request_id":Uuid::new_v4()});
    let response = context.resolve(&request).await;
    accepted(&context, &response, &request);
    assert_eq!(response.2["outcome"], "provider_unavailable");
    assert_eq!(response.2.get("failure"), Some(&case.failure()));
    assert_eq!(response.2["sources"], json!([]));
    assert_eq!(provider.requests_for(SEARCH_PATH).len(), 1);
    assert!(fixture::valid_search(
        &provider.requests_for(SEARCH_PATH)[0]
    ));
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(provider.requests_for(path).is_empty());
    }
    let failed_counts = counts(provider);
    let replay = context.resolve(&request).await;
    accepted(&context, &replay, &request);
    assert_eq!(replay.2, response.2);
    assert_eq!(counts(provider), failed_counts);
    assert_foreign_receipt_hidden(&context, &request).await;
    assert_eq!(counts(provider), failed_counts);
    assert_eq!(context.snapshot().await, before);
    assert_eq!(context.peer_snapshot().await, peer_before);
    assert_eq!(library.primary_counts().await, primary_before);
    assert_eq!(context.owner.get(&context.app, &choice_path).await, choice);

    let page = fixture::ready_search();
    provider.on(SEARCH_PATH, move |request| {
        if !fixture::valid_search(request) {
            return Reply::Status(StatusCode::BAD_REQUEST);
        }
        Reply::Json(page.clone())
    });
    let healthy_replay = context.resolve(&request).await;
    accepted(&context, &healthy_replay, &request);
    assert_eq!(healthy_replay.2, response.2);
    assert_eq!(counts(provider), failed_counts);
    let fresh_request = json!({"request_id":Uuid::new_v4()});
    let recovered = context.resolve(&fresh_request).await;
    accepted(&context, &recovered, &fresh_request);
    assert_eq!(recovered.2["outcome"], "ready");
    assert_eq!(recovered.2.get("failure"), Some(&Value::Null));
    assert_ne!(recovered.2["resolution_id"], response.2["resolution_id"]);
    let sources = recovered.2["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["recording_id"], context.recording);
    assert_eq!(sources[0]["provider"], "plugin");
    assert_eq!(sources[0]["label"], "Owned HTTP disposition audio");
    assert_eq!(sources[0]["ready"], true);
    assert_eq!(sources[0]["match_kind"], "verified");
    assert_eq!(
        context.owner.get(&context.app, &sources_path).await,
        recovered.2["sources"]
    );
    assert_eq!(provider.requests_for(SEARCH_PATH).len(), 2);
    assert!(
        provider
            .requests_for(SEARCH_PATH)
            .iter()
            .all(fixture::valid_search)
    );
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH] {
        let calls = provider.requests_for(path);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].json(), json!({"recording_id":fixture::CANDIDATE}));
        assert_eq!(calls[0].protocol().as_deref(), Some("1"));
    }
    let recovered_counts = counts(provider);
    let old_replay = context.resolve(&request).await;
    accepted(&context, &old_replay, &request);
    for field in [
        "request_id",
        "resolution_id",
        "recording_id",
        "outcome",
        "failure",
    ] {
        assert_eq!(old_replay.2[field], response.2[field]);
    }
    assert_eq!(counts(provider), recovered_counts);
    let fresh_replay = context.resolve(&fresh_request).await;
    accepted(&context, &fresh_replay, &fresh_request);
    assert_eq!(fresh_replay.2, recovered.2);
    assert_foreign_receipt_hidden(&context, &request).await;
    assert_foreign_receipt_hidden(&context, &fresh_request).await;
    assert_eq!(counts(provider), recovered_counts);
    let mut after = context.snapshot().await;
    let old_observations = before["metadata"]["observations"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(
        before["metadata"]["observations"].get("next_before"),
        Some(&Value::Null)
    );
    assert!(old_observations.len() < 20);
    assert!(
        old_observations
            .iter()
            .all(|observation| observation["provider"] != fixture::PROVIDER)
    );
    let observations = after["metadata"]["observations"]["items"]
        .as_array_mut()
        .unwrap();
    assert_eq!(observations.len(), old_observations.len() + 1);
    let matching = observations
        .iter()
        .enumerate()
        .filter_map(|(index, observation)| {
            (observation["provider"] == fixture::PROVIDER
                && observation["provider_recording_id"] == fixture::CANDIDATE
                && observation["provider_revision"] == "owned-http-disposition-r1")
                .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    let observation = &observations[matching[0]];
    assert_eq!(observation["title"], "Owned tune");
    assert_eq!(observation["artist"], "Owned performer");
    assert_eq!(observation["duration_ms"], 1000);
    observations.remove(matching[0]);
    assert_eq!(after, before);
    assert_eq!(context.peer_snapshot().await, peer_before);
    assert_eq!(library.primary_counts().await, primary_before);
    assert_eq!(
        context.owner.get(&context.app, &choice_path).await["mode"],
        choice["mode"]
    );
    assert_eq!(
        context.owner.get(&context.app, &choice_path).await["revision"],
        choice["revision"]
    );
    assert_eq!(provider.requests_for(MANIFEST_PATH).len(), 1);
    assert_eq!(provider.health_requests().len(), 1);
    assert!(provider.requests_for(AUDIO_PATH).is_empty());
    assert_eq!(provider.unexpected(), 0);
    assert!(
        provider
            .requests()
            .iter()
            .all(|request| request.handled == Handled::Yes)
    );
    let serialized = serde_json::to_string(&response.2).unwrap();
    for private in [
        fixture::PRIVATE_TITLE,
        fixture::REFUSAL,
        fixture::CANDIDATE,
        fixture::SOURCE,
        fixture::PROVIDER,
        library.primary.as_ref().unwrap().url.as_str(),
        provider.url.as_str(),
    ] {
        assert!(!serialized.contains(private));
    }
}

fn accepted(context: &Context, response: &Response, request: &Value) {
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1["cache-control"], "private, no-store");
    assert_eq!(response.2["request_id"], request["request_id"]);
    assert_eq!(response.2["recording_id"], context.recording);
    assert!(Uuid::parse_str(response.2["resolution_id"].as_str().unwrap()).is_ok());
    assert!(response.2.get("plugin_connection").is_none());
    assert!(
        !serde_json::to_string(&response.2)
            .unwrap()
            .contains("Owned performer Owned tune")
    );
}

async fn assert_foreign_receipt_hidden(context: &Context, request: &Value) {
    let denied = exchange(
        &context.app,
        "POST",
        &context.path(),
        request.clone(),
        context.peer.headers(),
    )
    .await;
    assert_eq!(denied.0, StatusCode::NOT_FOUND);
    assert_eq!(denied.1["cache-control"], "private, no-store");
    assert_eq!(denied.2, json!({"error":"Resource not found"}));
    for field in [
        "failure",
        "reason",
        "retryable",
        "plugin_connection",
        "sources",
        "resolution_id",
    ] {
        assert!(denied.2.get(field).is_none());
    }
    let detail = exchange(
        &context.app,
        "GET",
        &format!("/api/v1/recordings/{}/sources", context.recording),
        Value::Null,
        context.peer.headers(),
    )
    .await;
    assert_eq!(detail.0, StatusCode::NOT_FOUND);
    assert_eq!(detail.1["cache-control"], "private, no-store");
    assert_eq!(detail.2, denied.2);
}

fn counts(provider: &hito_testkit::fake_plugin::FakePlugin) -> (usize, usize) {
    (provider.requests().len(), provider.health_requests().len())
}
