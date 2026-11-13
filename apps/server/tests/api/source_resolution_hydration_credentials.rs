use crate::source_credential_fixture::{Context, OWNER_TOKEN, ProviderServer, installation_by_id};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::HANG_CEILING;
use hito_plugin_sdk::{
    AUDIO_PATH, DETAIL_PATH, MANIFEST_PATH, METADATA_PATH, SEARCH_PATH, SOURCES_PATH,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use uuid::Uuid;

use crate::source_authority_fixture as provider;

#[derive(Clone, Copy)]
enum RejectedExchange {
    Detail,
    Sources,
    Metadata,
}

impl RejectedExchange {
    fn path(self) -> &'static str {
        match self {
            Self::Detail => DETAIL_PATH,
            Self::Sources => SOURCES_PATH,
            Self::Metadata => METADATA_PATH,
        }
    }

    fn requested(self) -> &'static [&'static str] {
        match self {
            Self::Detail => &[DETAIL_PATH],
            Self::Sources => &[DETAIL_PATH, SOURCES_PATH],
            Self::Metadata => &[DETAIL_PATH, SOURCES_PATH, METADATA_PATH],
        }
    }

    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Detail => ("detail", "detail"),
            Self::Sources => ("list", "source-list"),
            Self::Metadata => ("metadata", "metadata"),
        }
    }
}

#[tokio::test]
async fn a_source_detail401_after_search200_diagnoses_only_its_current_owners_connection() {
    let current = observe_rejection(RejectedExchange::Detail).await;
    assert_eq!(
        current["health"]["state"], "credentials_required",
        "A current protected detail401 after search200 must update its owning connection's cached health"
    );
}

#[tokio::test]
async fn a_source_list401_after_detail200_diagnoses_only_its_current_owners_connection() {
    let current = observe_rejection(RejectedExchange::Sources).await;
    assert_eq!(
        current["health"]["state"], "credentials_required",
        "A current protected source-list401 after detail200 must update its owning connection's cached health"
    );
}

#[tokio::test]
async fn a_source_metadata401_after_sources200_diagnoses_only_its_current_owners_connection() {
    let current = observe_rejection(RejectedExchange::Metadata).await;
    assert_eq!(
        current["health"]["state"], "credentials_required",
        "A current protected metadata401 after sources200 must update its owning connection's cached health"
    );
}

async fn observe_rejection(stage: RejectedExchange) -> Value {
    let (suffix, label) = stage.labels();
    let primary = ProviderServer::start().await;
    let previous = ProviderServer::start().await;
    let first_id = format!("org.hito.source-{suffix}401");
    let next_id = format!("org.hito.source-{suffix}401-next");
    let mut first = provider::start(&first_id, true, OWNER_TOKEN).await;
    let mut next = provider::start(&next_id, false, OWNER_TOKEN).await;
    first.reply(stage.path(), Reply::Status(StatusCode::UNAUTHORIZED));
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(Box::pin(exercise(
            stage, &primary, &previous, &first, &next,
        )))
        .catch_unwind(),
    )
    .await;
    let first_stopped = first.finish().await;
    let next_stopped = next.finish().await;
    primary.shutdown().await;
    previous.shutdown().await;
    assert!(first_stopped && next_stopped);
    match result {
        Ok(Ok(current)) => current,
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("the owned {label} credential rejection must settle within its deadline"),
    }
}

async fn exercise(
    stage: RejectedExchange,
    primary: &ProviderServer,
    previous: &ProviderServer,
    first: &FakePlugin,
    next: &FakePlugin,
) -> Value {
    let started = Instant::now();
    let mut context = Context::prepare(primary, previous, first).await;
    let (_, label) = stage.labels();
    let offer_name = format!("Owned next {label} credential service");
    let next_installation = context
        .owner
        .connect(&context.app, &next.url, &offer_name, Some(OWNER_TOKEN))
        .await;
    context
        .refresh_peer_baseline_after_offering(next_installation["service_id"].as_str().unwrap())
        .await;
    let before = context.settings().await;
    assert_eq!(
        installation_by_id(&before, &context.installation["id"])["health"]["state"],
        "ok"
    );
    assert_eq!(
        installation_by_id(&before, &next_installation["id"])["health"]["state"],
        "ok"
    );
    assert!(first.requests_for(SEARCH_PATH).is_empty());
    for path in stage.requested() {
        assert!(first.requests_for(path).is_empty());
    }
    assert!(next.requests_for(SEARCH_PATH).is_empty());
    let request = json!({"request_id":Uuid::new_v4()});
    let response = context.resolve(&request).await;
    context.check_resolution(&response, &request);
    assert_eq!(response.2["outcome"], "provider_unavailable");
    let counts_before_replay = (
        first.requests().len(),
        next.requests().len(),
        first.health_requests().len(),
        next.health_requests().len(),
    );
    context.check_replay(&request, &response.2).await;
    assert_eq!(
        counts_before_replay,
        (
            first.requests().len(),
            next.requests().len(),
            first.health_requests().len(),
            next.health_requests().len(),
        )
    );
    let after = context
        .check_preserved(
            &before,
            &context.choice,
            primary,
            previous,
            first,
            &response.2,
        )
        .await;
    let mut expected = before.clone();
    let mut actual = after.clone();
    for settings in [&mut expected, &mut actual] {
        settings["installations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|item| item["id"] == context.installation["id"])
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("health");
    }
    assert_eq!(actual, expected);
    assert_eq!(
        installation_by_id(&after, &next_installation["id"]),
        installation_by_id(&before, &next_installation["id"]),
    );
    assert_eq!(first.requests_for(MANIFEST_PATH).len(), 2);
    assert_eq!(first.health_requests().len(), 2);
    assert_eq!(first.requests_for(SEARCH_PATH).len(), 1);
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH] {
        let calls = first.requests_for(path);
        let expected = usize::from(stage.requested().contains(&path));
        assert_eq!(calls.len(), expected, "{path}");
        if let Some(call) = calls.first() {
            assert_eq!(
                call.json(),
                json!({"recording_id":"owned-authority-candidate"})
            );
            assert_eq!(call.protocol().as_deref(), Some("1"));
            assert!(
                call.headers
                    .get("authorization")
                    .is_some_and(|value| value == format!("Bearer {OWNER_TOKEN}").as_str())
            );
        }
    }
    assert_eq!(next.requests_for(MANIFEST_PATH).len(), 1);
    assert_eq!(next.health_requests().len(), 1);
    assert_eq!(next.requests_for(SEARCH_PATH).len(), 1);
    assert!(first.requests_for(AUDIO_PATH).is_empty());
    for path in [DETAIL_PATH, SOURCES_PATH, METADATA_PATH, AUDIO_PATH] {
        assert!(next.requests_for(path).is_empty());
    }
    assert_eq!(first.unexpected(), 0);
    assert_eq!(next.unexpected(), 0);
    assert!(
        !serde_json::to_string(&response.2)
            .unwrap()
            .contains(next.url.as_str())
    );
    assert!(started.elapsed() < Duration::from_secs(20));
    installation_by_id(&after, &context.installation["id"]).clone()
}
