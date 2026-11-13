use crate::{
    account::exchange,
    catalog_album_header_fixture as catalog,
    member_search_credential_order_fixture::{self as fixture, Context, Library, Response},
    member_search_credential_provider::{self as provider, SearchOutcome},
    member_search_dispatch_gate::DispatchPause,
};
use axum::http::{Method, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use hito_testkit::fake_plugin::Reply;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_success_dispatched_last_clears_an_intervening_group_rejection() {
    let observed = run(SearchOutcome::Available).await;
    assert_eq!(
        observed.current, observed.baseline,
        "The later dispatched checked member success must clear the intervening credential rejection"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_rejection_dispatched_last_overrides_an_intervening_group_success() {
    let observed = run(SearchOutcome::Rejected).await;
    let mut expected = observed.baseline;
    expected["health"] = json!({"state": "credentials_required", "reason": null, "since": null});
    assert_eq!(
        observed.current, expected,
        "The later dispatched checked member rejection must determine current credential health"
    );
}

struct Observed {
    baseline: Value,
    current: Value,
}

async fn run(mode: SearchOutcome) -> Observed {
    let mut library = Library::new();
    let pause = DispatchPause::default();
    let mut requests = JoinSet::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "member credential ordering owned setup",
                Box::pin(library.prepare(mode)),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(
                &library,
                context.as_ref().unwrap(),
                &pause,
                &mut requests,
                mode,
            ),
        )
        .await
        .expect("The member dispatch ordering workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    pause.release();
    requests.abort_all();
    let drained = tokio::time::timeout(Duration::from_secs(3), async {
        while requests.join_next().await.is_some() {}
    })
    .await;
    drop(context.take());
    library.retire().await;
    assert!(drained.is_ok(), "All owned ordering HTTP tasks must join");
    match outcome {
        Ok(observed) => observed,
        Err(panic) => resume_unwind(panic),
    }
}

async fn exercise(
    library: &Library,
    context: &Context,
    pause: &DispatchPause,
    requests: &mut JoinSet<Response>,
    mode: SearchOutcome,
) -> Observed {
    let provider = library.provider.as_ref().unwrap();
    let before = context.snapshot().await;
    let peer_before = context.peer_snapshot().await;
    let origin_before = library.origin_counts().await;
    let provider_before = provider.requests().len();
    let path = context.search_path();
    let body = context.search_body();
    let denied = exchange(
        &context.app,
        "POST",
        &path,
        body.clone(),
        context.peer.headers(),
    )
    .await;
    assert_eq!(denied.0, StatusCode::NOT_FOUND);
    fixture::private(&denied.1);
    assert_eq!(denied.2, json!({"error": "Resource not found"}));
    assert_eq!(provider.requests().len(), provider_before);
    let app = context.app.clone();
    let headers = context.owner.headers();
    hito_testkit::logs::anchor_callsite_interest();
    let subscriber = tracing_subscriber::registry().with(pause.clone());
    requests.spawn(
        async move { exchange(&app, "POST", &path, body, headers).await }
            .with_subscriber(subscriber),
    );
    pause.entered().await;
    assert!(requests.try_join_next().is_none());
    assert_eq!(provider.requests_for(provider::SEARCH).len(), 0);
    assert_eq!(
        provider.requests()[provider_before..]
            .iter()
            .map(|request| request.operation.as_str())
            .collect::<Vec<_>>(),
        [provider::DISCOVERY],
    );
    match mode {
        SearchOutcome::Available => {
            provider.reply(
                catalog::GROUP_DETAIL,
                Reply::Status(StatusCode::UNAUTHORIZED),
            );
            let rejection = context.group().await;
            fixture::rejected(&rejection, &context.installation);
            assert_eq!(
                context.current_installation().await["health"],
                json!({"state": "credentials_required", "reason": null, "since": null})
            );
        }
        SearchOutcome::Rejected => {
            let available = context.group().await;
            assert_eq!(available.0, StatusCode::OK);
            fixture::private(&available.1);
            assert_eq!(
                available.2,
                json!({"status": "available",
                "association_id": context.association["id"], "group": catalog::group()})
            );
            assert_eq!(context.current_installation().await, context.installation);
        }
    }
    pause.assert_held();
    assert!(requests.try_join_next().is_none());
    assert!(provider.requests_for(provider::SEARCH).is_empty());
    assert_eq!(provider.requests_for(catalog::GROUP_DETAIL).len(), 1);
    pause.release();
    let search = tokio::time::timeout(Duration::from_secs(3), requests.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    pause.assert_returned();
    assert!(requests.is_empty());
    match mode {
        SearchOutcome::Available => {
            assert_eq!(search.0, StatusCode::OK);
            fixture::private(&search.1);
            assert_eq!(
                search.2,
                json!({"status": "available", "context": context.context,
                "page": provider::page()})
            );
        }
        SearchOutcome::Rejected => fixture::rejected(&search, &context.installation),
    }
    context.assert_current_context().await;
    assert_eq!(context.snapshot().await, before);
    assert_eq!(context.peer_snapshot().await, peer_before);
    assert_eq!(library.origin_counts().await, origin_before);
    let recorded = provider.requests();
    let dispatched = &recorded[provider_before..];
    assert_eq!(
        dispatched
            .iter()
            .map(|request| (&request.method, request.operation.as_str()))
            .collect::<Vec<_>>(),
        [
            (&Method::GET, provider::DISCOVERY),
            (&Method::GET, catalog::GROUP_CAPABILITIES),
            (&Method::POST, catalog::GROUP_DETAIL),
            (&Method::POST, provider::SEARCH),
        ]
    );
    for request in dispatched {
        assert_eq!(request.protocol().as_deref(), Some("1"));
        assert!(request.query.is_none());
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {}", provider::TOKEN).as_str()
        );
    }
    assert_eq!(
        dispatched[2].json(),
        json!({"release_group_id": catalog::GROUP})
    );
    assert_eq!(dispatched[3].json(), json!(provider::request()));
    assert_eq!(provider.unexpected(), 0);
    let current = context.current_installation().await;
    assert_eq!(provider.requests().len(), recorded.len());
    Observed {
        baseline: context.installation.clone(),
        current,
    }
}
