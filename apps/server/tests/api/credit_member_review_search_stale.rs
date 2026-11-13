use crate::{
    account::exchange,
    artist_review_fixture::origin_counts,
    credit_member_review_credentials as fixture,
    credit_member_review_credentials_provider as provider,
    credit_member_review_owned::{Library, Response},
};
use axum::http::{Method, StatusCode};
use futures_util::FutureExt;
use hito_core::{Action, Operation, test_support::hang_guard::hang_guard};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn a_member_search_response_cannot_clear_credentials_after_its_featured_credit_is_removed() {
    let mut library = Library::retain();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let context = hang_guard(
            "owned stale member search setup",
            Box::pin(async {
                library.open().await;
                Box::pin(fixture::prepare(&library)).await
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&mut library, &context)),
        )
        .await
        .expect("The stale member response workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.close().await;
    let observed = match outcome {
        Ok(observed) => observed,
        Err(panic) => resume_unwind(panic),
    };
    fixture::private(&observed.response.1);
    assert_eq!(observed.response.0, StatusCode::CONFLICT);
    assert_eq!(observed.response.2.as_object().unwrap().len(), 1);
    assert!(
        observed.response.2["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty() && error.len() <= 240)
    );
    assert!(observed.response.2.get("plugin_connection").is_none());
    assert!(observed.response.2.get("code").is_none());
    assert!(observed.response.2.get("page").is_none());
    assert!(observed.response.2.get("status").is_none());
    assert_eq!(
        observed.current, observed.required,
        "A stale protected success must not clear the current credential diagnosis"
    );
}

struct Observed {
    response: Response,
    required: Value,
    current: Value,
}

async fn exercise(library: &mut Library, context: &fixture::Context) -> Observed {
    let before = context.snapshot(library).await;
    let origin_before = origin_counts(library.origin()).await;
    let calls_before = library.plugin().requests().len();
    let denied = exchange(
        library.app(),
        "POST",
        &context.path(),
        json!({"context": context.context, "search": provider::query()}),
        context.peer.headers(),
    )
    .await;
    fixture::private(&denied.1);
    assert_eq!(denied.0, StatusCode::NOT_FOUND);
    assert_eq!(library.plugin().requests().len(), calls_before);
    let baseline = context.owner.get(library.app(), "/api/v1/plugins").await;
    assert_eq!(
        fixture::installation(&baseline, &context.installation["id"])["health"],
        json!({"state": "ok", "reason": null, "since": null})
    );
    provider::reject(library.plugin(), provider::SEARCH);
    let rejected = exchange(
        library.app(),
        "POST",
        &context.path(),
        json!({"context": context.context, "search": provider::query()}),
        context.owner.headers(),
    )
    .await;
    fixture::private(&rejected.1);
    assert_eq!(rejected.0, StatusCode::CONFLICT);
    assert_eq!(rejected.2["code"], "plugin_credentials_required");
    assert_eq!(
        rejected.2["plugin_connection"],
        json!({
            "installation_id": context.installation["id"],
            "installation_revision": context.installation["revision"],
        })
    );
    assert_eq!(rejected.2.as_object().unwrap().len(), 3);
    let mut required = baseline;
    let row = required["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == context.installation["id"])
        .unwrap();
    row["health"] = json!({"state": "credentials_required", "reason": null, "since": null});
    assert_eq!(
        context.owner.get(library.app(), "/api/v1/plugins").await,
        required
    );
    context.assert_private(library).await;
    provider::valid_search(library.plugin());
    library.hold_search(
        context.owner.headers(),
        context.path(),
        json!({"context": context.context, "search": provider::query()}),
    );
    assert!(library.search_entered().await);
    assert!(library.search_pending());
    assert_eq!(
        context.owner.get(library.app(), "/api/v1/plugins").await,
        required
    );
    let operation = Operation {
        id: Uuid::new_v4(),
        action: Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&context.recording).unwrap(),
            artist: "Ada".into(),
        },
    };
    let accepted = fixture::accepted(
        library.app(),
        &context.owner,
        "POST",
        "/api/v1/sync",
        json!({"device_id": "owned-member-search-role-loss", "operations": [&operation]}),
    )
    .await;
    assert_eq!(accepted["acknowledged"], json!([operation.id]));
    let post_edit = context.snapshot(library).await;
    assert_eq!(post_edit["peer"], before["peer"]);
    assert_eq!(post_edit["owner"]["metadata"]["artist"]["personal"], "Ada");
    assert_eq!(
        post_edit["owner"]["metadata"]["artist"]["operation_id"],
        json!(operation.id)
    );
    assert!(
        post_edit["owner"]["metadata"]["credited_artists"]
            .as_array()
            .unwrap()
            .iter()
            .all(|member| member["artist_id"] != context.member)
    );
    assert_eq!(
        post_edit["owner"]["metadata"]["selected_observation"],
        before["owner"]["metadata"]["selected_observation"]
    );
    assert_eq!(
        post_edit["owner"]["metadata"]["observations"],
        before["owner"]["metadata"]["observations"]
    );
    assert_eq!(
        post_edit["owner"]["metadata"]["title"],
        before["owner"]["metadata"]["title"]
    );
    assert_eq!(post_edit["owner"]["sources"], before["owner"]["sources"]);
    assert_eq!(post_edit["owner"]["progress"], before["owner"]["progress"]);
    assert!(library.search_pending());
    let response = library.complete_search().await;
    assert_eq!(
        context.snapshot(library).await,
        post_edit,
        "The stale response must preserve the legitimate post-edit state"
    );
    let current = context.owner.get(library.app(), "/api/v1/plugins").await;
    assert_eq!(origin_counts(library.origin()).await, origin_before);
    let calls: Vec<_> = library.plugin().requests()[calls_before..]
        .iter()
        .map(|request| {
            assert!(provider::owner_request(request));
            (
                request.method.clone(),
                request.operation.clone(),
                request.json(),
            )
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            (Method::GET, provider::DESCRIPTOR.into(), Value::Null),
            (Method::POST, provider::SEARCH.into(), provider::query()),
            (Method::GET, provider::DESCRIPTOR.into(), Value::Null),
            (Method::POST, provider::SEARCH.into(), provider::query()),
        ]
    );
    assert_eq!(library.plugin().unexpected(), 0);
    Observed {
        response,
        required,
        current,
    }
}
