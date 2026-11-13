use crate::{
    artist_identity_fixture::Library, artist_identity_inertia_fixture::page,
    artist_review_fixture::exchange,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

const RELEASE_GROUP: &str = "group:owned/á?album=一&literal=%2F#edition";
const ENCODED_RELEASE_GROUP: &str =
    "group%3Aowned%2F%C3%A1%3Falbum%3D%E4%B8%80%26literal%3D%252F%23edition";

#[tokio::test]
async fn a_cold_catalog_album_page_preserves_its_decoded_release_group_and_private_authority() {
    let library = hang_guard("cold catalog album library", Box::pin(Library::prepare())).await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let (_, receipt) = hang_guard(
            "cold catalog album reviewed imported identity",
            Box::pin(library.select(&library.imported)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&library, &receipt)),
        )
        .await
        .expect("The cold catalog album Inertia workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.finish().await;
    let response = match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(response["props"]["release_group_id"], RELEASE_GROUP);
}

async fn exercise(library: &Library, receipt: &Value) -> Value {
    let selection = &receipt["selection"];
    let group = selection["group_id"].as_str().unwrap();
    assert!(Uuid::parse_str(group).is_ok());
    let detail = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/artist-identity-groups/{group}?limit=1"),
        )
        .await;
    assert_eq!(detail["group"]["id"], group);
    assert_eq!(detail["group"]["name"]["recording_id"], library.imported);
    assert_eq!(
        detail["group"]["name"]["association_id"],
        selection["association_id"]
    );
    assert_eq!(detail["group"]["provider"], json!(library.provider));
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let path = format!("/artist-groups/{group}/releases/{ENCODED_RELEASE_GROUP}");
    let mut peer = library.peer.headers();
    peer.insert("x-inertia", "true".parse().unwrap());
    let (status, headers, denied) = exchange(&library.app, "GET", &path, Value::Null, peer).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
    assert_eq!(denied, json!({"error": "Resource not found"}));
    assert_eq!(library.provider_counts().await, provider_before);
    let response = page(&library.app, &library.owner, &path, None, false).await;
    assert_eq!(response["component"], "CatalogAlbumPage");
    assert_eq!(response["props"]["user"]["id"], library.owner.id);
    assert_eq!(response["props"]["detail"], detail);
    assert_eq!(library.provider_counts().await, provider_before);
    assert_eq!(library.snapshot().await, before);
    response
}

#[tokio::test]
async fn catalog_album_transport_preserves_partial_fields_and_rejects_malformed_segments() {
    let library = hang_guard(
        "catalog album transport library",
        Box::pin(Library::prepare()),
    )
    .await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let (_, receipt) = hang_guard(
            "catalog album transport reviewed imported identity",
            Box::pin(library.select(&library.imported)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(transport_controls(&library, &receipt)),
        )
        .await
        .expect("The catalog album transport workflow must finish within its deadline");
    }))
    .catch_unwind()
    .await;
    library.finish().await;
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn transport_controls(library: &Library, receipt: &Value) {
    let group = receipt["selection"]["group_id"].as_str().unwrap();
    let detail = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/artist-identity-groups/{group}?limit=1"),
        )
        .await;
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let base = format!("/artist-groups/{group}/releases/");
    let path = format!("{base}{ENCODED_RELEASE_GROUP}");
    for (fields, excluded, expected) in [
        (
            "release_group_id",
            None,
            json!({"errors": {}, "release_group_id": RELEASE_GROUP}),
        ),
        ("detail", None, json!({"errors": {}, "detail": detail})),
        (
            "release_group_id,detail",
            Some("release_group_id"),
            json!({"errors": {}, "detail": detail}),
        ),
    ] {
        let mut headers = library.owner.headers();
        headers.insert("x-inertia", "true".parse().unwrap());
        headers.insert(
            "x-inertia-partial-component",
            "CatalogAlbumPage".parse().unwrap(),
        );
        headers.insert("x-inertia-partial-data", fields.parse().unwrap());
        if let Some(excluded) = excluded {
            headers.insert("x-inertia-partial-except", excluded.parse().unwrap());
        }
        let (status, headers, response) =
            exchange(&library.app, "GET", &path, Value::Null, headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["cache-control"], "private, no-store");
        assert_eq!(headers["vary"], "X-Inertia");
        assert_eq!(headers["x-inertia"], "true");
        assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
        assert_eq!(response["component"], "CatalogAlbumPage");
        assert_eq!(response["url"], path);
        assert_eq!(response["props"], expected);
        assert_eq!(library.provider_counts().await, provider_before);
    }
    let plus = page(
        &library.app,
        &library.owner,
        &format!("{base}group+owned%2Fedition"),
        None,
        false,
    )
    .await;
    assert_eq!(plus["component"], "CatalogAlbumPage");
    assert_eq!(plus["props"]["release_group_id"], "group+owned/edition");
    assert_eq!(plus["props"]["detail"], detail);
    assert_eq!(library.provider_counts().await, provider_before);
    for (segment, expected) in [
        ("group%", StatusCode::UNPROCESSABLE_ENTITY),
        ("group%G0", StatusCode::UNPROCESSABLE_ENTITY),
        ("group%FF", StatusCode::UNPROCESSABLE_ENTITY),
        ("group%00", StatusCode::UNPROCESSABLE_ENTITY),
        ("%20%20", StatusCode::UNPROCESSABLE_ENTITY),
        ("", StatusCode::NOT_FOUND),
        ("group/extra", StatusCode::NOT_FOUND),
    ] {
        let mut headers = library.owner.headers();
        headers.insert("x-inertia", "true".parse().unwrap());
        let (status, headers, response) = exchange(
            &library.app,
            "GET",
            &format!("{base}{segment}"),
            Value::Null,
            headers,
        )
        .await;
        assert_eq!(status, expected);
        assert_eq!(headers["cache-control"], "private, no-store");
        assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
        if expected == StatusCode::UNPROCESSABLE_ENTITY {
            assert_eq!(
                response,
                json!({"error": "Use a valid catalog release-group path"})
            );
        } else {
            assert_eq!(response["component"], "NotFoundPage");
        }
        assert_eq!(library.provider_counts().await, provider_before);
    }
    let mut peer_results = Vec::new();
    for (included, excluded) in [
        (Some("release_group_id"), None),
        (Some("user"), None),
        (None, Some("detail")),
    ] {
        let mut headers = library.peer.headers();
        headers.insert("x-inertia", "true".parse().unwrap());
        headers.insert(
            "x-inertia-partial-component",
            "CatalogAlbumPage".parse().unwrap(),
        );
        if let Some(included) = included {
            headers.insert("x-inertia-partial-data", included.parse().unwrap());
        }
        if let Some(excluded) = excluded {
            headers.insert("x-inertia-partial-except", excluded.parse().unwrap());
        }
        let (status, headers, response) =
            exchange(&library.app, "GET", &path, Value::Null, headers).await;
        assert_eq!(headers["cache-control"], "private, no-store");
        assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
        assert_eq!(library.provider_counts().await, provider_before);
        peer_results.push((status, response));
    }
    assert_eq!(library.snapshot().await, before);
    for (status, response) in peer_results {
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(response, json!({"error": "Resource not found"}));
    }
}
