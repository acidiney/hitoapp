#[path = "../../../hito-core/tests/fixtures/content_service.rs"]
mod fixture;
use super::support::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn provider_search_selection_and_audio_form_one_private_http_workflow() {
    let server = fixture::ProviderServer::start().await;
    let (dir, app) = app().await;
    let token = admin(&app).await;
    let original = write_wav(&dir.path().join("remote.wav"));
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name":"Fixture","base_url":server.url}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status,installed) = call(&app,"POST","/api/v1/plugins",json!({"service_id":service["id"],"service_revision":1,"bearer_token":"private-provider-token"}),Some(&token)).await;
    assert_eq!(status, StatusCode::CREATED);
    let details = json!({"id":"remote/item","title":"Remote track","artist":"Remote artist","duration_ms":1000});
    server
        .reply("v1/search", json!({"items":[details.clone()]}))
        .await;
    server.reply("v1/recordings/detail", details).await;
    server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"remote/source","label":"Original","ready":true}]}),
        )
        .await;
    server
        .audio(
            StatusCode::OK,
            axum::http::HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/wav".parse().unwrap(),
            )]),
            original.clone(),
        )
        .await;
    let search = format!(
        "/api/v1/plugins/{}/search",
        installed["id"].as_str().unwrap()
    );
    let body = json!({"installation_revision":1,"query":"Remote","cursor":null,"limit":20});
    let (status, results) = call(&app, "POST", &search, body.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(results["items"][0]["id"], "remote/item");
    assert_eq!(
        call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token))
            .await
            .1["total"],
        0
    );
    let selection = format!(
        "/api/v1/plugins/{}/recordings",
        installed["id"].as_str().unwrap()
    );
    let selected = json!({"installation_revision":1,"provider_recording_id":"remote/item"});
    let (status, recording) = call(&app, "POST", &selection, selected.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recording["title"], "Remote track");
    assert_eq!(
        call(&app, "POST", &selection, selected.clone(), Some(&token))
            .await
            .1["id"],
        recording["id"]
    );
    let (_, sources) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/recordings/{}/sources",
            recording["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    let audio = format!(
        "/api/v1/sources/{}/audio",
        sources[0]["id"].as_str().unwrap()
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&audio)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        original
    );
    call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"bob","password":"provider bob password"}),
        Some(&token),
    )
    .await;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"bob","password":"provider bob password"}),
        None,
    )
    .await;
    let bob = session["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "POST", &search, body, Some(bob)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "POST", &selection, selected, Some(bob)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", &audio, Value::Null, Some(bob)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(server.requests("v1/audio").await.len(), 1);
}
