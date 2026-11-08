use crate::{
    artist_identity_fixture::Library, artist_identity_inertia_fixture::page,
    artist_review_fixture::exchange,
};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn inertia_identity_groups_keep_session_account_and_current_grants_for_full_partial_and_html_reads()
 {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (local, receipt) = library.select(&library.local).await;
    library.select(&library.imported).await;
    let group = receipt["selection"]["group_id"].as_str().unwrap();
    let path = format!("/artist-groups/{group}");
    let shared = local["context"]["library_id"].as_str().unwrap();
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;

    for html in [false, true] {
        let index = page(&library.app, &library.peer, "/artists", None, html).await;
        assert_eq!(index["component"], "ArtistIndexPage");
        assert_eq!(
            index["props"]["identity_groups"],
            json!({"items": [], "total": 0, "offset": 0, "limit": 60})
        );
        assert_eq!(
            index["props"]["artists"],
            library
                .peer
                .get(&library.app, "/api/v1/artists?sort=recent")
                .await
        );
    }
    for route in ["/artists", path.as_str()] {
        let mut anonymous = HeaderMap::new();
        anonymous.insert("x-inertia", "true".parse().unwrap());
        anonymous.insert(
            "x-inertia-partial-component",
            "ArtistIdentityGroupPage".parse().unwrap(),
        );
        anonymous.insert("x-inertia-partial-data", "detail".parse().unwrap());
        let (status, headers, login) =
            exchange(&library.app, "GET", route, Value::Null, anonymous).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["cache-control"], "private, no-store");
        assert_eq!(login["component"], "LoginPage");
        assert_eq!(login["props"], json!({"errors": {}, "needs_setup": false}));
        assert_eq!(login["clearHistory"], true);
        let mut mismatched = library.owner.headers();
        mismatched.insert("x-hito-account", library.peer.id.parse().unwrap());
        let (status, _, error) =
            exchange(&library.app, "GET", route, Value::Null, mismatched).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(error["error"].is_string());
    }
    for (html, partial) in [
        (true, None),
        (false, None),
        (false, Some("ArtistIdentityGroupPage")),
        (false, Some("PlaylistPage")),
    ] {
        let mut headers = library.peer.headers();
        if !html {
            headers.insert("x-inertia", "true".parse().unwrap());
        }
        if let Some(component) = partial {
            headers.insert("x-inertia-partial-component", component.parse().unwrap());
            headers.insert("x-inertia-partial-data", "detail".parse().unwrap());
        }
        let (status, headers, error) =
            exchange(&library.app, "GET", &path, Value::Null, headers).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error": "Resource not found"}));
        assert_eq!(headers["cache-control"], "private, no-store");
        assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
    }
    let access = format!("/api/v1/libraries/{shared}/members/{}", library.owner.id);
    assert_eq!(
        library
            .owner
            .request(&library.app, "DELETE", &access, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let expected = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/artist-identity-groups/{group}"),
        )
        .await;
    assert_eq!(expected["group"]["library_count"], 1);
    assert_eq!(expected["group"]["recording_count"], 1);
    assert_eq!(expected["group"]["release_count"], 1);
    assert_eq!(expected["group"]["name"]["recording_id"], library.imported);
    assert_eq!(
        expected["recordings"]["items"][0]["recording"]["id"],
        library.imported
    );
    for html in [false, true] {
        assert_eq!(
            page(&library.app, &library.owner, &path, None, html).await["props"]["detail"],
            expected
        );
    }
    let partial = page(
        &library.app,
        &library.owner,
        &path,
        Some(("ArtistIdentityGroupPage", "detail")),
        false,
    )
    .await;
    assert_eq!(partial["props"], json!({"errors": {}, "detail": expected}));
    let scoped_path = format!("{path}?library={shared}");
    for html in [false, true] {
        let mut headers = library.owner.headers();
        if !html {
            headers.insert("x-inertia", "true".parse().unwrap());
        }
        let (status, _, error) =
            exchange(&library.app, "GET", &scoped_path, Value::Null, headers).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error": "Resource not found"}));
    }
    let index = page(
        &library.app,
        &library.owner,
        &format!("/artists?library={shared}"),
        None,
        false,
    )
    .await;
    assert_eq!(index["props"]["artists"]["total"], 0);
    assert_eq!(index["props"]["identity_groups"]["total"], 0);
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &access, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        page(&library.app, &library.owner, &path, None, false).await["props"]["detail"]["group"]["recording_count"],
        2
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
