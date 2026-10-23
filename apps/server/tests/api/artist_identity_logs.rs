use crate::{
    artist_identity_fixture::{ARTIST_ID, Library},
    artist_identity_logs_fixture::Logs,
    artist_review_fixture::{self as review, exchange},
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::ChooseRecordingArtistIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    panic::{AssertUnwindSafe, resume_unwind},
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

struct Observed {
    request_id: String,
    method: &'static str,
    route: &'static str,
    status: u16,
}

#[tokio::test]
async fn http_identity_workflow_logs_correlate_safe_outcomes_and_durations_without_private_inputs()
{
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (context, association) = library.review(&library.local).await;
    let choose = json!(ChooseRecordingArtistIdentity {
        request_id: Uuid::new_v4(),
        expected_revision: 0,
        association_id: Uuid::parse_str(&association).unwrap(),
        context,
    });
    let cookie = library.owner.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let canaries = vec![
        cookie.clone(),
        token.to_owned(),
        format!("{:x}", Sha256::digest(token.as_bytes())),
        library.audio.to_string_lossy().into_owned(),
        "owned-identity.wav".into(),
        "Owned local HTTP recording".into(),
        "Private HTTP title pin".into(),
        "Private HTTP artist review selection".into(),
        review::CREDIT.into(),
        ARTIST_ID.into(),
        "private-identity-search-canary".into(),
        choose.to_string(),
        json!(library.provider)["installation_id"]
            .as_str()
            .unwrap()
            .to_owned(),
    ];
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let logs = Logs::default();
    let observed = async {
        let mut observed = Vec::new();
        let path = format!("/api/v1/recordings/{}/artist-identity", library.local);
        let (_, call) = observe(
            library,
            "GET",
            &path,
            "/api/v1/recordings/{id}/artist-identity",
            Value::Null,
            StatusCode::OK,
        )
        .await;
        observed.push(call);
        let (receipt, call) = observe(
            library,
            "PUT",
            &path,
            "/api/v1/recordings/{id}/artist-identity",
            choose.clone(),
            StatusCode::OK,
        )
        .await;
        observed.push(call);
        let (_, call) = observe(
            library,
            "GET",
            "/api/v1/artist-identity-groups",
            "/api/v1/artist-identity-groups",
            Value::Null,
            StatusCode::OK,
        )
        .await;
        observed.push(call);
        let detail = format!(
            "/api/v1/artist-identity-groups/{}",
            receipt["selection"]["group_id"].as_str().unwrap()
        );
        let (_, call) = observe(
            library,
            "GET",
            &detail,
            "/api/v1/artist-identity-groups/{id}",
            Value::Null,
            StatusCode::OK,
        )
        .await;
        observed.push(call);
        let mut stale = choose;
        stale["request_id"] = json!(Uuid::new_v4());
        let (_, call) = observe(
            library,
            "PUT",
            &path,
            "/api/v1/recordings/{id}/artist-identity",
            stale,
            StatusCode::CONFLICT,
        )
        .await;
        observed.push(call);
        let (_, call) = observe(
            library,
            "POST",
            &format!("{path}/reset"),
            "/api/v1/recordings/{id}/artist-identity/reset",
            json!({"request_id": Uuid::new_v4(), "expected_revision": 1}),
            StatusCode::OK,
        )
        .await;
        observed.push(call);
        let (_, call) = observe(
            library,
            "GET",
            "/api/v1/artist-identity-groups?q=private-identity-search-canary",
            "/api/v1/artist-identity-groups",
            Value::Null,
            StatusCode::OK,
        )
        .await;
        observed.push(call);
        observed
    }
    .with_subscriber(logs.subscriber())
    .await;
    let events = logs.assert_private(&canaries);
    let ids = observed
        .iter()
        .map(|call| call.request_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 7);
    let responses = events
        .iter()
        .filter(|event| event["fields"]["event"] == "http_response")
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), observed.len());
    for call in observed {
        let matched = responses
            .iter()
            .filter(|event| event["span"]["request_id"] == call.request_id)
            .collect::<Vec<_>>();
        assert_eq!(matched.len(), 1);
        let event = matched[0];
        assert_eq!(event["span"]["route"], call.route);
        assert_eq!(event["span"]["method"], call.method);
        assert_eq!(event["fields"]["status"], call.status);
        assert!(event["fields"]["duration_ms"].as_u64().is_some());
    }
    for (message, operation, outcome, count) in [
        (
            "Artist identity choice HTTP request completed",
            "http_artist_identity_choice",
            "returned",
            1,
        ),
        (
            "Artist identity selection HTTP request completed",
            "http_artist_identity_choose",
            "accepted",
            1,
        ),
        (
            "Artist identity selection HTTP request completed",
            "http_artist_identity_choose",
            "conflict",
            1,
        ),
        (
            "Artist identity groups HTTP request completed",
            "http_artist_identity_groups",
            "returned",
            2,
        ),
        (
            "Artist identity group HTTP request completed",
            "http_artist_identity_group",
            "returned",
            1,
        ),
        (
            "Artist identity reset HTTP request completed",
            "http_artist_identity_reset",
            "accepted",
            1,
        ),
    ] {
        let matching = events
            .iter()
            .filter(|event| {
                event["fields"]["message"] == message && event["fields"]["outcome"] == outcome
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), count);
        for event in matching {
            assert_eq!(event["span"]["operation"], operation);
            Uuid::parse_str(event["span"]["request_id"].as_str().unwrap()).unwrap();
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            let parent = event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|span| span["name"] == "http_request")
                .unwrap();
            assert!(ids.contains(parent["request_id"].as_str().unwrap()));
        }
    }
    assert_eq!(
        library
            .owner
            .get(
                &library.app,
                &format!("/api/v1/recordings/{}/artist-identity", library.local)
            )
            .await["revision"],
        2
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}

async fn observe(
    library: &Library,
    method: &'static str,
    uri: &str,
    route: &'static str,
    body: Value,
    expected: StatusCode,
) -> (Value, Observed) {
    let (status, headers, value) =
        exchange(&library.app, method, uri, body, library.owner.headers()).await;
    assert_eq!(status, expected);
    let request_id = headers["x-request-id"].to_str().unwrap().to_owned();
    Uuid::parse_str(&request_id).unwrap();
    (
        value,
        Observed {
            request_id,
            method,
            route,
            status: status.as_u16(),
        },
    )
}
