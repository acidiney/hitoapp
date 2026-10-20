use crate::{artist_identity_fixture::Library, artist_review_fixture::exchange};
use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn http_identity_routes_require_the_session_account_and_current_library_grant() {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (request, receipt) = library.select(&library.local).await;
    let group = receipt["selection"]["group_id"].as_str().unwrap();
    let path = format!("/api/v1/recordings/{}/artist-identity", library.local);
    let private = format!("/api/v1/recordings/{}/artist-identity", library.imported);
    let detail = format!("/api/v1/artist-identity-groups/{group}");
    let reset = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let mut anonymous = library.owner.headers();
    anonymous.remove("cookie");
    let mut wrong_account = library.owner.headers();
    wrong_account.insert("x-hito-account", library.peer.id.parse().unwrap());
    for (method, route, body) in [
        ("GET", path.clone(), Value::Null),
        ("PUT", path.clone(), request.clone()),
        ("POST", format!("{path}/reset"), reset.clone()),
        ("GET", "/api/v1/artist-identity-groups".into(), Value::Null),
        ("GET", detail.clone(), Value::Null),
    ] {
        for (headers, expected) in [
            (anonymous.clone(), StatusCode::UNAUTHORIZED),
            (wrong_account.clone(), StatusCode::FORBIDDEN),
        ] {
            let (status, _, _) =
                exchange(&library.app, method, &route, body.clone(), headers).await;
            assert_eq!(status, expected);
        }
    }
    assert_eq!(
        library.peer.get(&library.app, &path).await,
        json!({"recording_id": library.local, "revision": 0, "state": {"status": "unselected"}})
    );
    assert_eq!(
        library
            .peer
            .get(&library.app, "/api/v1/artist-identity-groups")
            .await["total"],
        0
    );
    for (method, route, body) in [
        ("GET", private.clone(), Value::Null),
        ("GET", detail.clone(), Value::Null),
        ("PUT", path.clone(), request.clone()),
        ("POST", format!("{private}/reset"), reset.clone()),
    ] {
        assert_eq!(
            library
                .peer
                .request(&library.app, method, &route, body)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &private, request.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let shared = request["context"]["library_id"].as_str().unwrap();
    let access = format!("/api/v1/libraries/{shared}/members/{}", library.owner.id);
    assert_eq!(
        library
            .owner
            .request(&library.app, "DELETE", &access, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for (method, route, body) in [
        ("GET", path.clone(), Value::Null),
        ("PUT", path.clone(), request.clone()),
        ("POST", format!("{path}/reset"), reset),
        ("GET", detail.clone(), Value::Null),
    ] {
        assert_eq!(
            library
                .owner
                .request(&library.app, method, &route, body)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        library
            .owner
            .get(&library.app, "/api/v1/artist-identity-groups")
            .await["total"],
        0
    );
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &access, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, replay) = library
        .owner
        .request(&library.app, "PUT", &path, request.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, receipt);
    let mut stale = request;
    stale["request_id"] = json!(Uuid::new_v4());
    stale["expected_revision"] = json!(1);
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &path, stale)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        library.owner.get(&library.app, &path).await,
        json!({"recording_id": library.local, "revision": 1, "state": {"status": "selected", "group_id": group, "association_id": receipt["selection"]["association_id"]}})
    );
    assert_eq!(
        library.owner.get(&library.app, &detail).await["group"]["recording_count"],
        1
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
