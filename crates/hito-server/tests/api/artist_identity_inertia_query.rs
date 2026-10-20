use crate::{
    artist_identity_fixture::Library, artist_identity_inertia_fixture::page,
    artist_review_fixture::exchange,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};

#[tokio::test]
async fn inertia_identity_group_queries_bound_pages_and_evaluate_only_matching_requested_props() {
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
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let bounded = page(
        &library.app,
        &library.owner,
        "/artists?offset=-4&group_offset=-3&limit=1000",
        None,
        false,
    )
    .await;
    for field in ["artists", "identity_groups"] {
        assert_eq!(bounded["props"][field]["offset"], 0);
        assert_eq!(bounded["props"][field]["limit"], 200);
    }
    let bounded = page(
        &library.app,
        &library.owner,
        &format!(
            "{detail_path}?library_offset=-1&recording_offset=-2&release_offset=-3&limit=1000"
        ),
        None,
        false,
    )
    .await;
    for field in ["libraries", "recordings", "releases"] {
        assert_eq!(bounded["props"]["detail"][field]["offset"], 0);
        assert_eq!(bounded["props"]["detail"][field]["limit"], 200);
    }
    let minimal = page(
        &library.app,
        &library.owner,
        &format!("{detail_path}?limit=0"),
        None,
        false,
    )
    .await;
    for field in ["libraries", "recordings", "releases"] {
        assert_eq!(minimal["props"]["detail"][field]["limit"], 1);
        assert_eq!(
            minimal["props"]["detail"][field]["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    let empty = page(
        &library.app,
        &library.owner,
        "/artists?group_offset=9223372036854775807",
        Some(("ArtistIndexPage", "identity_groups")),
        false,
    )
    .await;
    assert_eq!(empty["props"]["identity_groups"]["offset"], i64::MAX);
    assert_eq!(empty["props"]["identity_groups"]["total"], 1);
    assert_eq!(empty["props"]["identity_groups"]["items"], json!([]));
    for path in [
        "/artists?group_offset=invalid".into(),
        "/artists?group_offset=9223372036854775808".into(),
        "/artists?limit=invalid".into(),
        format!("{detail_path}?library_offset=invalid"),
        format!("{detail_path}?recording_offset=invalid"),
        format!("{detail_path}?release_offset=invalid"),
    ] {
        let mut headers = library.owner.headers();
        headers.insert("x-inertia", "true".parse().unwrap());
        let (status, _, error) = exchange(&library.app, "GET", &path, Value::Null, headers).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            error,
            json!({"error": "Request query does not match the expected fields"})
        );
    }
    let long_query = "private-inertia-query-canary".repeat(20);
    assert!(long_query.len() > 512);
    let path = format!("/artists?q={long_query}");
    for component in [None, Some("ArtistIndexPage"), Some("AlbumIndexPage")] {
        let mut headers = library.owner.headers();
        headers.insert("x-inertia", "true".parse().unwrap());
        if let Some(component) = component {
            headers.insert("x-inertia-partial-component", component.parse().unwrap());
            headers.insert("x-inertia-partial-data", "identity_groups".parse().unwrap());
        }
        let (status, _, error) = exchange(&library.app, "GET", &path, Value::Null, headers).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(error["error"].is_string());
        assert!(!error.to_string().contains(&long_query));
    }
    let query = page(
        &library.app,
        &library.owner,
        &path,
        Some(("ArtistIndexPage", "query")),
        false,
    )
    .await;
    assert_eq!(query["props"], json!({"errors": {}, "query": long_query}));
    let mut headers = library.owner.headers();
    headers.insert("x-inertia", "true".parse().unwrap());
    headers.insert(
        "x-inertia-partial-component",
        "ArtistIndexPage".parse().unwrap(),
    );
    headers.insert(
        "x-inertia-partial-data",
        "query,identity_groups".parse().unwrap(),
    );
    headers.insert(
        "x-inertia-partial-except",
        "identity_groups".parse().unwrap(),
    );
    let (status, _, omitted) = exchange(&library.app, "GET", &path, Value::Null, headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(omitted["props"], json!({"errors": {}, "query": long_query}));
    let user = page(
        &library.app,
        &library.owner,
        "/artist-groups/nonexistent",
        Some(("ArtistIdentityGroupPage", "user")),
        false,
    )
    .await;
    assert_eq!(user["component"], "ArtistIdentityGroupPage");
    let keys = user["props"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(keys, vec!["errors", "user"]);
    assert_eq!(user["props"]["user"]["id"], library.owner.id);
    let mut headers = library.owner.headers();
    headers.insert("x-inertia", "true".parse().unwrap());
    headers.insert(
        "x-inertia-partial-component",
        "PlaylistPage".parse().unwrap(),
    );
    headers.insert("x-inertia-partial-data", "user".parse().unwrap());
    let (status, _, error) = exchange(
        &library.app,
        "GET",
        "/artist-groups/nonexistent",
        Value::Null,
        headers,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error, json!({"error": "Resource not found"}));
    let mut headers = library.owner.headers();
    headers.insert("x-inertia", "true".parse().unwrap());
    headers.insert(
        "x-inertia-version",
        "owned-outdated-version".parse().unwrap(),
    );
    let (status, response, body) = exchange(&library.app, "GET", &path, Value::Null, headers).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(response["x-inertia-location"], path);
    assert_eq!(body, Value::Null);
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
