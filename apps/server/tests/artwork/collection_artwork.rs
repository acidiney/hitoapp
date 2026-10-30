use hito_core::test_support::collection_artwork as provider;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{Actor, ImportProviderPlaylist, PluginInstallation, Services};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::{admin, call};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn album_artwork_requires_approved_private_authority_and_repairs_existing_copies() {
    let directory = tempfile::tempdir().unwrap();
    let hito = support::services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let actor = hito.identity.authenticate(&token).await.unwrap();
    let provider = provider::ArtworkServer::start_without_artwork().await;
    let installed = provider.connect(&hito, &actor).await;
    let first = import_copy(&hito, &actor, &installed).await;
    let second = import_copy(&hito, &actor, &installed).await;
    assert_ne!(first, second);
    let catalog = get(&app, &token, "/api/v1/recordings").await;
    assert_eq!(catalog["total"], 1);
    let recording = catalog["items"][0]["id"].as_str().unwrap();
    let release = catalog["items"][0]["release_id"].as_str().unwrap();
    let image = format!("/api/v1/releases/{release}/artwork");
    let availability = format!("{image}/availability");
    let fetch = format!("{image}/fetch");
    let operations = [
        json!({"kind": "recording_title_set", "recording_id": recording, "title": "My retained recording"}),
        json!({"kind": "release_title_set", "release_id": release, "title": "My retained album"}),
        json!({"kind": "like_set", "recording_id": recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": recording, "position_ms": 4200, "speed": 1.25}),
        json!({"kind": "playlist_rename", "playlist_id": first, "name": "My retained copy"}),
    ].into_iter().map(|action| json!({"id": Uuid::new_v4(), "action": action})).collect::<Vec<_>>();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "collection-artwork-http", "operations": operations,
            }),
            Some(&token)
        )
        .await
        .0,
        StatusCode::OK
    );
    let before = saved_state(&app, &token, [first, second], recording).await;

    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch": false})
    );
    assert_eq!(
        call(&app, "POST", &fetch, Value::Null, Some(&token))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    provider.enable_artwork();
    let refresh = format!("/api/v1/plugins/{}/refresh", installed.id);
    let (status, review) = call(
        &app,
        "POST",
        &refresh,
        json!({"revision": installed.revision}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch": false})
    );
    assert_eq!(
        call(&app, "POST", &fetch, Value::Null, Some(&token))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(provider.requests().await.is_empty());
    assert!(
        !hito
            .catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    let (status, approval) = call(
        &app,
        "POST",
        &refresh,
        json!({
            "revision": installed.revision, "accept_manifest_digest": review["digest"],
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(approval["kind"], "updated");
    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch": true})
    );

    let credentials = json!({"username": "other", "password": "other collection artwork password"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/users",
            credentials.clone(),
            Some(&token)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let (status, other_session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other = other_session["token"].as_str().unwrap();
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other), StatusCode::NOT_FOUND),
    ] {
        for (method, path) in [("GET", &image), ("GET", &availability), ("POST", &fetch)] {
            assert_eq!(
                call(&app, method, path, Value::Null, token).await.0,
                expected
            );
        }
    }
    let (status, queued) = call(&app, "POST", &fetch, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(queued["release_id"], release);
    assert!(queued.get("recording_id").is_none());
    assert_eq!(queued["status"], "queued");
    let job = format!(
        "/api/v1/release-artwork-fetches/{}",
        queued["id"].as_str().unwrap()
    );
    assert_eq!(get(&app, &token, &job).await, queued);
    assert_eq!(
        call(&app, "POST", &fetch, Value::Null, Some(&token)).await,
        (StatusCode::ACCEPTED, queued.clone())
    );
    assert!(provider.requests().await.is_empty());
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    let ready = get(&app, &token, &job).await;
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["attempts"], 1);
    assert_eq!(
        provider.requests().await,
        [ArtworkRequest {
            entity: ArtworkEntity::Release,
            provider_id: provider::RELEASE_ID.into(),
        }]
    );
    assert_image(&app, &image, &token).await;
    assert_eq!(
        saved_state(&app, &token, [first, second], recording).await,
        before
    );
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other), StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            call(&app, "GET", &job, Value::Null, token).await.0,
            expected
        );
    }

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/plugins/{}", installed.id),
            json!({
                "revision": approval["installation"]["revision"],
            }),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch": false})
    );
    assert_image(&app, &image, &token).await;
    assert_eq!(get(&app, &token, &job).await, ready);
    assert_eq!(
        call(&app, "POST", &fetch, Value::Null, Some(&token)).await,
        (StatusCode::ACCEPTED, ready)
    );
    assert_eq!(provider.requests().await.len(), 1);
    assert_eq!(
        saved_state(&app, &token, [first, second], recording).await,
        before
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for (method, path) in [
        ("GET", &image),
        ("GET", &availability),
        ("GET", &job),
        ("POST", &fetch),
    ] {
        assert_eq!(
            call(&app, method, path, Value::Null, Some(&token)).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}

async fn import_copy(hito: &Services, actor: &Actor, installed: &PluginInstallation) -> Uuid {
    hito.collection
        .import_provider_playlist(
            actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id
}

async fn get(app: &Router, token: &str, path: &str) -> Value {
    let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn assert_image(app: &Router, path: &str, token: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        provider::RELEASE_COVER
    );
}

async fn saved_state(app: &Router, token: &str, copies: [Uuid; 2], recording: &str) -> Value {
    json!({
        "catalog": get(app, token, "/api/v1/recordings").await,
        "albums": get(app, token, "/api/v1/releases").await,
        "metadata": get(app, token, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "playlists": get(app, token, "/api/v1/playlists").await,
        "first": get(app, token, &format!("/api/v1/playlists/{}", copies[0])).await,
        "second": get(app, token, &format!("/api/v1/playlists/{}", copies[1])).await,
        "activity": get(app, token, "/api/v1/activity").await,
        "events": get(app, token, "/api/v1/sync?cursor=0").await,
    })
}
