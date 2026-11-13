use crate::{
    account::exchange,
    credit_member_review_preview_fixture::{Context, accepted, private},
    credit_member_review_preview_security_fixture::{self as fixture, Owned, Response},
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{
    Action, Operation,
    test_support::{artist_catalog_discovery_service as catalog, hang_guard::hang_guard},
};
use hito_plugin_sdk::ArtistProfileRequest;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

struct Observed {
    response: Response,
    required: Value,
    current: Value,
}

#[tokio::test]
async fn a_held_member_profile_cannot_clear_credentials_after_its_featured_target_is_removed() {
    let mut owned = Owned::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "owned held member preview setup",
                Box::pin(owned.library.prepare()),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(stale_profile(&mut owned, context.as_ref().unwrap())),
        )
        .await
        .expect("The held member preview workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    owned.close().await;
    let observed = match outcome {
        Ok(observed) => observed,
        Err(panic) => resume_unwind(panic),
    };
    private(&observed.response.1);
    assert_eq!(observed.response.0, StatusCode::CONFLICT);
    assert_eq!(observed.response.2.as_object().unwrap().len(), 1);
    assert!(
        observed.response.2["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty() && error.len() <= 240)
    );
    for field in ["code", "plugin_connection", "profile", "context", "status"] {
        assert!(
            observed.response.2.get(field).is_none(),
            "A stale member preview must not publish profile or credential evidence"
        );
    }
    assert!(!observed.response.2.to_string().contains(catalog::TOKEN));
    assert!(
        !observed
            .response
            .2
            .to_string()
            .contains(catalog::SECOND_ARTIST)
    );
    assert!(
        observed.current == observed.required,
        "The stale protected profile must retain the current credential warning"
    );
}

#[tokio::test]
async fn a_mismatched_profile_success_cannot_clear_the_current_member_credential_warning() {
    let mut owned = Owned::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "owned malformed member preview setup",
                Box::pin(owned.library.prepare()),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(mismatched_profile(&owned, context.as_ref().unwrap())),
        )
        .await
        .expect("The malformed member profile workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    owned.close().await;
    let observed = match outcome {
        Ok(observed) => observed,
        Err(panic) => resume_unwind(panic),
    };
    private(&observed.response.1);
    assert_eq!(observed.response.0, StatusCode::OK);
    assert!(
        observed.response.2 == json!({"status": "unavailable"}),
        "A different artist's successful profile must not be exposed for the requested identity"
    );
    assert!(
        observed.current == observed.required,
        "A mismatched profile must not clear the current credential warning"
    );
}

async fn stale_profile(owned: &mut Owned, context: &Context) -> Observed {
    let initial = context.snapshot().await;
    let playlist = initial["library"]["playlist"]["playlist"]["id"]
        .as_str()
        .unwrap();
    let before = fixture::state(context, playlist).await;
    let plugins_before = context.account.get(&context.app, "/api/v1/plugins").await;
    let peer_before = context.peer_snapshot().await;
    let origin_before = owned.library.origin_counts().await;
    let calls_before = owned.provider().requests().len();
    let denied = exchange(
        &context.app,
        "POST",
        &fixture::path(context),
        fixture::body(context),
        context.peer.headers(),
    )
    .await;
    private(&denied.1);
    assert_eq!(denied.0, StatusCode::NOT_FOUND);
    assert!(denied.2 == json!({"error": "Resource not found"}));
    assert_eq!(owned.provider().requests().len(), calls_before);
    let required = fixture::diagnose(owned, context, &plugins_before).await;
    assert!(
        fixture::state(context, playlist).await == before,
        "Diagnosing the owned profile must preserve the whole choice and private collection"
    );
    owned.hold(context);
    assert!(
        owned.entered().await,
        "The valid owned profile body must actually be held"
    );
    assert!(owned.pending());
    assert!(
        context.account.get(&context.app, "/api/v1/plugins").await == required,
        "Descriptor and partial-body success must not clear credentials"
    );
    let operation = Operation {
        id: Uuid::new_v4(),
        action: Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(context.context["recording_id"].as_str().unwrap())
                .unwrap(),
            artist: "Owned ordering lead".into(),
        },
    };
    let edited = accepted(
        &context.app,
        &context.account,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "owned-member-preview-role-loss", "operations": [&operation],
        }),
    )
    .await;
    assert!(edited["acknowledged"] == json!([operation.id]));
    let post_edit = fixture::state(context, playlist).await;
    assert!(post_edit["metadata"]["artist"]["personal"] == "Owned ordering lead");
    assert!(post_edit["metadata"]["artist"]["operation_id"] == json!(operation.id));
    assert!(
        post_edit["metadata"]["credited_artists"]
            .as_array()
            .unwrap()
            .iter()
            .all(|member| member["artist_id"] != context.context["artist_id"])
    );
    for field in ["selected_observation", "observations", "title"] {
        assert!(
            post_edit["metadata"][field] == before["metadata"][field],
            "The public artist edit must retain unrelated recording evidence"
        );
    }
    for field in ["sources", "progress", "settings"] {
        assert!(
            post_edit[field] == before[field],
            "The public artist edit must retain unrelated private state"
        );
    }
    assert!(context.peer_snapshot().await == peer_before);
    assert!(owned.pending());
    assert!(
        context.account.get(&context.app, "/api/v1/plugins").await == required,
        "The legitimate role edit must retain the current credential warning before profile release"
    );
    let response = owned.complete().await;
    assert!(
        fixture::state(context, playlist).await == post_edit,
        "The stale response must preserve the complete legitimate post-edit whole-choice and collection state"
    );
    assert!(context.peer_snapshot().await == peer_before);
    assert!(owned.library.origin_counts().await == origin_before);
    fixture::protocol(owned, calls_before);
    let current = context.account.get(&context.app, "/api/v1/plugins").await;
    Observed {
        response,
        required,
        current,
    }
}

async fn mismatched_profile(owned: &Owned, context: &Context) -> Observed {
    let initial = context.snapshot().await;
    let playlist = initial["library"]["playlist"]["playlist"]["id"]
        .as_str()
        .unwrap();
    let before = fixture::state(context, playlist).await;
    let plugins_before = context.account.get(&context.app, "/api/v1/plugins").await;
    let peer_before = context.peer_snapshot().await;
    let origin_before = owned.library.origin_counts().await;
    let calls_before = owned.provider().requests().len();
    let required = fixture::diagnose(owned, context, &plugins_before).await;
    let mut mismatched = catalog::expected_profile();
    mismatched.artist.id = catalog::FIRST_ARTIST.into();
    assert!(
        mismatched
            .validate_for(&ArtistProfileRequest {
                artist_id: catalog::FIRST_ARTIST.into()
            })
            .is_ok()
    );
    assert!(
        mismatched
            .validate_for(&ArtistProfileRequest {
                artist_id: catalog::SECOND_ARTIST.into()
            })
            .is_err()
    );
    owned.provider().mismatched_profile();
    let response = exchange(
        &context.app,
        "POST",
        &fixture::path(context),
        fixture::body(context),
        context.account.headers(),
    )
    .await;
    assert!(
        fixture::state(context, playlist).await == before,
        "The malformed profile must preserve the original whole choice and private collection"
    );
    assert!(context.peer_snapshot().await == peer_before);
    assert!(owned.library.origin_counts().await == origin_before);
    fixture::protocol(owned, calls_before);
    let current = context.account.get(&context.app, "/api/v1/plugins").await;
    Observed {
        response,
        required,
        current,
    }
}
