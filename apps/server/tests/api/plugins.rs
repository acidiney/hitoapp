use super::support::*;
use axum::http::StatusCode;
use hito_core::test_support::plugin_service as fixture;
use serde_json::{Value, json};

#[tokio::test]
async fn plugin_http_routes_enforce_personal_ownership_and_administrator_offers() {
    let server = fixture::PluginServer::start().await;
    let (_dir, app) = app().await;
    let admin = admin(&app).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/plugins", Value::Null, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            json!({"username":"bob","password":"bob plugin password"}),
            Some(&admin)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let bob = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"bob","password":"bob plugin password"}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let offer = json!({"name":"Fixture","base_url":server.url});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/plugins/services",
            offer.clone(),
            Some(&bob)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        offer,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let secret = "http-private-plugin-token";
    server.require_bearer(secret).await;
    let (status, installed) = call(&app, "POST", "/api/v1/plugins", json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":secret}), Some(&bob)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(installed["credential_set"], true);
    assert!(!installed.to_string().contains(secret));
    let (_, settings) = call(&app, "GET", "/api/v1/plugins", Value::Null, Some(&admin)).await;
    assert_eq!(settings["installations"], json!([]));
    let id = installed["id"].as_str().unwrap();
    for (method, suffix, request) in [
        ("POST", "/refresh", json!({"revision":1})),
        ("POST", "/declaration-check", json!({"revision":1})),
        ("DELETE", "", json!({"revision":1})),
        ("DELETE", "/credential", json!({"revision":1})),
    ] {
        assert_eq!(
            call(
                &app,
                method,
                &format!("/api/v1/plugins/{id}{suffix}"),
                request,
                Some(&admin)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/plugins/{id}/refresh"),
            json!({"revision":1,"base_url":"http://unoffered.invalid/"}),
            Some(&bob)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(server.hits(), 1);
    let (status, updated) = call(
        &app,
        "POST",
        &format!("/api/v1/plugins/{id}/refresh"),
        json!({"revision":1}),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["kind"], "updated");
    let (status, checked) = call(
        &app,
        "POST",
        &format!("/api/v1/plugins/{id}/declaration-check"),
        json!({"revision":2}),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(checked, json!({"kind":"current"}));
    let (status, cleared) = call(
        &app,
        "DELETE",
        &format!("/api/v1/plugins/{id}/credential"),
        json!({"revision":2}),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["credential_set"], false);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/plugins/{id}"),
            json!({"revision":2}),
            Some(&bob)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/plugins/{id}"),
            json!({"revision":3}),
            Some(&bob)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let service_id = service["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/plugins/services/{service_id}"),
            json!({"revision":1}),
            Some(&admin)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[test]
fn generated_operations_have_unique_identifiers() {
    let document = serde_json::to_value(hito_server::openapi()).unwrap();
    let mut identifiers = std::collections::HashSet::new();
    for path in document["paths"].as_object().unwrap().values() {
        for method in ["get", "post", "put", "patch", "delete"] {
            if let Some(id) = path[method]["operationId"].as_str() {
                assert!(
                    identifiers.insert(id),
                    "Duplicate operation identifier: {id}"
                );
            }
        }
    }
}
