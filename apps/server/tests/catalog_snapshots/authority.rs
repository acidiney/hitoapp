use super::fixture::{Fixture, PASSWORD, ROOT, raw, request};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};

#[tokio::test]
async fn snapshot_routes_require_current_unlocked_sessions_and_matching_accounts() {
    let f = Fixture::start().await;
    let snapshot = f.begin().await;
    let login = request(
        &f.app,
        "POST",
        "/api/v1/login",
        json!({"username":"admin", "password":PASSWORD}),
        None,
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);
    let second_token = login.json()["token"].as_str().unwrap().to_owned();
    let id = snapshot["id"].as_str().unwrap();
    let routes = [
        ("POST", ROOT.to_owned()),
        ("GET", format!("{ROOT}/{id}/pages/0")),
        ("DELETE", format!("{ROOT}/{id}")),
    ];
    for (method, path) in &routes {
        let absent = request(&f.app, method, path, json!({"library_id":f.library}), None).await;
        assert_eq!(absent.status, StatusCode::UNAUTHORIZED);
        absent.private();
        let mismatched = raw(
            &f.app,
            Request::builder()
                .method(*method)
                .uri(path)
                .header("authorization", format!("Bearer {}", f.token))
                .header("x-hito-account", "different-account")
                .header("content-type", "application/json")
                .body(Body::from(json!({"library_id":f.library}).to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(mismatched.status, StatusCode::FORBIDDEN);
    }
    assert_eq!(
        f.call(
            "PUT",
            "/api/v1/account/pin",
            json!({"password":PASSWORD,"pin":"1234","idle_minutes":0})
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        f.call("POST", "/api/v1/account/lock", Value::Null)
            .await
            .status,
        StatusCode::OK
    );
    for (method, path) in &routes {
        let locked = f.call(method, path, json!({"library_id":f.library})).await;
        assert_eq!(locked.status, StatusCode::LOCKED);
        assert_eq!(locked.json()["code"], "locked");
    }
    assert_eq!(
        f.call("POST", "/api/v1/account/unlock", json!({"pin":"1234"}))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", &routes[1].1, Value::Null).await.status,
        StatusCode::OK
    );
    assert_eq!(
        f.call("POST", "/api/v1/logout", Value::Null).await.status,
        StatusCode::NO_CONTENT
    );
    for (method, path) in &routes {
        assert_eq!(
            f.call(method, path, json!({"library_id":f.library}))
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
    let resumed = request(
        &f.app,
        "POST",
        ROOT,
        json!({"library_id":f.library}),
        Some(&second_token),
    )
    .await;
    assert_eq!(resumed.status, StatusCode::OK);
    assert_eq!(resumed.json(), snapshot);
    assert_eq!(
        request(
            &f.app,
            "GET",
            &routes[1].1,
            Value::Null,
            Some(&second_token)
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &f.app,
            "DELETE",
            &routes[2].1,
            Value::Null,
            Some(&second_token)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn foreign_cleanup_is_idempotent_and_revoked_library_owners_can_release() {
    let f = Fixture::start().await;
    let owner = f.member("reader").await;
    let stranger = f.member("stranger").await;
    let body = json!({"library_id":f.library});
    assert_eq!(
        request(&f.app, "POST", ROOT, body.clone(), Some(&owner.token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    f.access(&owner.id, true).await;
    f.access(&stranger.id, true).await;
    let started = request(&f.app, "POST", ROOT, body.clone(), Some(&owner.token)).await;
    assert_eq!(started.status, StatusCode::OK);
    let snapshot = started.json();
    let id = snapshot["id"].as_str().unwrap();
    let page = format!("{ROOT}/{id}/pages/0");
    let release = format!("{ROOT}/{id}");
    assert_eq!(
        request(&f.app, "GET", &page, Value::Null, Some(&stranger.token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let foreign = request(
        &f.app,
        "DELETE",
        &release,
        Value::Null,
        Some(&stranger.token),
    )
    .await;
    assert_eq!(foreign.status, StatusCode::NO_CONTENT);
    assert!(foreign.bytes.is_empty());
    assert_eq!(
        request(&f.app, "GET", &page, Value::Null, Some(&owner.token))
            .await
            .status,
        StatusCode::OK
    );
    f.access(&owner.id, false).await;
    assert_eq!(
        request(&f.app, "POST", ROOT, body.clone(), Some(&owner.token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&f.app, "GET", &page, Value::Null, Some(&owner.token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let own_cleanup = request(&f.app, "DELETE", &release, Value::Null, Some(&owner.token)).await;
    assert_eq!(own_cleanup.status, StatusCode::NO_CONTENT);
    assert!(own_cleanup.bytes.is_empty());
    f.access(&owner.id, true).await;
    let new = request(&f.app, "POST", ROOT, body.clone(), Some(&owner.token)).await;
    assert_eq!(new.status, StatusCode::OK);
    let new = new.json();
    assert_ne!(new["id"], snapshot["id"]);
    f.access(&owner.id, false).await;
    f.access(&owner.id, true).await;
    assert_eq!(
        request(
            &f.app,
            "GET",
            &format!("{ROOT}/{}/pages/0", new["id"].as_str().unwrap()),
            Value::Null,
            Some(&owner.token)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&f.app, "POST", ROOT, body, Some(&owner.token))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        request(&f.app, "GET", &page, Value::Null, Some(&owner.token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let unknown = f
        .call(
            "DELETE",
            &format!("{ROOT}/{}", uuid::Uuid::new_v4()),
            Value::Null,
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NO_CONTENT);
    assert!(unknown.bytes.is_empty());
    f.services.catalog_replication.close().await.unwrap();
}
