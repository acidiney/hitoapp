use crate::{
    artist_identity_fixture::{ARTIST_ID, Library},
    artist_identity_logs_fixture::Logs,
    artist_review_fixture as review,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use futures_util::FutureExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

struct Observed {
    request_id: String,
    route: &'static str,
    status: u16,
}

#[tokio::test]
async fn inertia_group_logs_keep_safe_correlated_outcomes_and_timings_without_private_inputs() {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (_, receipt) = library.select(&library.local).await;
    library.select(&library.imported).await;
    let group = receipt["selection"]["group_id"].as_str().unwrap();
    let detail_path = format!("/artist-groups/{group}");
    let cookie = library.owner.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let canary = "private-inertia-log-search-canary";
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
        canary.into(),
        group.into(),
        library.local.clone(),
        json!(library.provider)["installation_id"]
            .as_str()
            .unwrap()
            .to_owned(),
    ];
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let logs = Logs::default();
    let observed = async {
        let mut owner = library.owner.headers();
        owner.insert("x-inertia", "true".parse().unwrap());
        let mut calls = Vec::new();
        calls.push(
            observe(
                &library.app,
                owner.clone(),
                "/artists?group_offset=0&limit=1",
                "/artists",
                StatusCode::OK,
            )
            .await,
        );
        calls.push(
            observe(
                &library.app,
                library.owner.headers(),
                &detail_path,
                "/artist-groups/{id}",
                StatusCode::OK,
            )
            .await,
        );
        let mut partial = owner.clone();
        partial.insert(
            "x-inertia-partial-component",
            "ArtistIdentityGroupPage".parse().unwrap(),
        );
        partial.insert("x-inertia-partial-data", "detail".parse().unwrap());
        calls.push(
            observe(
                &library.app,
                partial,
                &detail_path,
                "/artist-groups/{id}",
                StatusCode::OK,
            )
            .await,
        );
        let mut peer = library.peer.headers();
        peer.insert("x-inertia", "true".parse().unwrap());
        calls.push(
            observe(
                &library.app,
                peer,
                &detail_path,
                "/artist-groups/{id}",
                StatusCode::NOT_FOUND,
            )
            .await,
        );
        let mut query = owner.clone();
        query.insert(
            "x-inertia-partial-component",
            "ArtistIndexPage".parse().unwrap(),
        );
        query.insert("x-inertia-partial-data", "query".parse().unwrap());
        calls.push(
            observe(
                &library.app,
                query,
                &format!("/artists?q={}", canary.repeat(20)),
                "/artists",
                StatusCode::OK,
            )
            .await,
        );
        calls.push(
            observe(
                &library.app,
                owner.clone(),
                &format!("/artists?q={canary}"),
                "/artists",
                StatusCode::OK,
            )
            .await,
        );
        calls.push(
            observe(
                &library.app,
                owner.clone(),
                "/artist-groups/nonexistent",
                "/artist-groups/{id}",
                StatusCode::NOT_FOUND,
            )
            .await,
        );
        calls.push(
            observe(
                &library.app,
                owner,
                "/artists?group_offset=invalid",
                "/artists",
                StatusCode::BAD_REQUEST,
            )
            .await,
        );
        calls
    }
    .with_subscriber(logs.subscriber())
    .await;
    let events = logs.assert_private(&canaries);
    let ids = observed
        .iter()
        .map(|call| call.request_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 8);
    let responses = events
        .iter()
        .filter(|event| event["fields"]["event"] == "http_response")
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), observed.len());
    for call in &observed {
        let matching = responses
            .iter()
            .filter(|event| event["span"]["request_id"] == call.request_id)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        let event = matching[0];
        assert_eq!(event["span"]["route"], call.route);
        assert_eq!(event["span"]["method"], "GET");
        assert_eq!(event["fields"]["status"], call.status);
        assert!(event["fields"]["duration_ms"].as_u64().is_some());
    }
    for (message, operation, outcome, count) in [
        (
            "Artist identity groups loaded",
            "artist_identity_groups",
            "success",
            2,
        ),
        (
            "Artist identity group loaded",
            "artist_identity_group",
            "success",
            2,
        ),
        (
            "Artist identity group loaded",
            "artist_identity_group",
            "not_found",
            2,
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
            assert!(Uuid::parse_str(event["span"]["request_id"].as_str().unwrap()).is_ok());
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
    let assembled = events
        .iter()
        .filter(|event| event["fields"]["event"] == "inertia_page")
        .collect::<Vec<_>>();
    assert_eq!(assembled.len(), 5);
    for event in assembled {
        assert!(
            ["ArtistIndexPage", "ArtistIdentityGroupPage"]
                .contains(&event["fields"]["component"].as_str().unwrap())
        );
        assert!(event["fields"]["partial"].is_boolean());
    }
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}

async fn observe(
    app: &Router,
    headers: HeaderMap,
    path: &str,
    route: &'static str,
    status: StatusCode,
) -> Observed {
    let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
    *request.headers_mut() = headers;
    let response = tokio::time::timeout(Duration::from_secs(5), app.clone().oneshot(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let request_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(Uuid::parse_str(&request_id).is_ok());
    assert!(
        !to_bytes(response.into_body(), 524_288)
            .await
            .unwrap()
            .is_empty()
    );
    Observed {
        request_id,
        route,
        status: status.as_u16(),
    }
}
