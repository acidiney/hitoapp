use hito_core::test_support::collection_artwork as provider;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{
    Actor, FollowProviderPlaylist, ImportProviderPlaylist, PlaylistFollowInterval,
    PluginInstallation, Services,
};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::{admin, call};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn copied_playlist_covers_keep_exact_private_identity_receipts_and_saved_state() {
    let directory = tempfile::tempdir().unwrap();
    let hito = support::services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let actor = hito.identity.authenticate(&token).await.unwrap();
    let provider = provider::ArtworkServer::start_without_artwork().await;
    let installed = provider.connect(&hito, &actor).await;
    let copies = [
        import_copy(&hito, &actor, &installed).await,
        import_copy(&hito, &actor, &installed).await,
    ];
    assert_ne!(copies[0], copies[1]);
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installed.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/an-unrelated-submitted-reference"
                    .into(),
                interval: PlaylistFollowInterval::Daily,
            },
        )
        .await
        .unwrap()
        .playlist_id;
    let manual = Uuid::new_v4();
    let catalog = get(&app, &token, "/api/v1/recordings").await;
    assert_eq!(catalog["total"], 1);
    let recording = catalog["items"][0]["id"].as_str().unwrap();
    let operations = [
        json!({"kind":"playlist_create","playlist_id":manual,"name":"My local selection"}),
        json!({"kind":"playlist_rename","playlist_id":copies[0],"name":"My first private copy"}),
        json!({"kind":"playlist_rename","playlist_id":copies[1],"name":"My second private copy"}),
        json!({"kind":"recording_title_set","recording_id":recording,"title":"My private recording"}),
        json!({"kind":"like_set","recording_id":recording,"liked":true}),
        json!({"kind":"progress_set","recording_id":recording,"position_ms":4200,"speed":1.25}),
    ].into_iter().map(|action| json!({"id":Uuid::new_v4(),"action":action})).collect::<Vec<_>>();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/sync",
            json!({"device_id":"playlist-artwork-http","operations":operations}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::OK
    );
    let before = saved_state(&app, &token, copies, recording).await;
    let image = format!("/api/v1/playlists/{}/artwork", copies[0]);
    let availability = format!("{image}/availability");
    let fetch = format!("{image}/fetch");

    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch":false})
    );
    assert_eq!(
        call(&app, "POST", &fetch, Value::Null, Some(&token))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(provider.requests().await.is_empty());
    provider.enable_artwork();
    let refresh = format!("/api/v1/plugins/{}/refresh", installed.id);
    let (status, review) = call(
        &app,
        "POST",
        &refresh,
        json!({"revision":installed.revision}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["kind"], "review_required");
    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch":false})
    );
    assert_eq!(
        call(&app, "POST", &fetch, Value::Null, Some(&token))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (status, approved) = call(
        &app,
        "POST",
        &refresh,
        json!({"revision":installed.revision,"accept_manifest_digest":review["digest"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(approved["kind"], "updated");
    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch":true})
    );
    for id in [manual, followed] {
        let image = format!("/api/v1/playlists/{id}/artwork");
        assert_eq!(
            get(&app, &token, &format!("{image}/availability")).await,
            json!({"can_fetch":false})
        );
        assert_eq!(
            call(&app, "GET", &image, Value::Null, Some(&token)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("{image}/fetch"),
                Value::Null,
                Some(&token)
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let credentials =
        json!({"username":"other","password":"other owned playlist artwork password"});
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
    let (status, session) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    assert_eq!(status, StatusCode::OK);
    let other = session["token"].as_str().unwrap();
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other), StatusCode::NOT_FOUND),
    ] {
        for (method, path) in [
            ("GET", image.as_str()),
            ("GET", availability.as_str()),
            ("POST", fetch.as_str()),
        ] {
            assert_eq!(
                call(&app, method, path, Value::Null, token).await.0,
                expected
            );
        }
    }

    let mut receipts = Vec::new();
    for copy in copies {
        let image = format!("/api/v1/playlists/{copy}/artwork");
        let fetch = format!("{image}/fetch");
        assert_eq!(
            call(&app, "GET", &image, Value::Null, Some(&token)).await.0,
            StatusCode::NOT_FOUND
        );
        let (status, queued) = call(&app, "POST", &fetch, Value::Null, Some(&token)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(queued["playlist_id"], copy.to_string());
        assert_eq!(queued["status"], "queued");
        assert!(queued.get("recording_id").is_none() && queued.get("release_id").is_none());
        let id = queued["id"].as_str().unwrap();
        let receipt = format!("/api/v1/playlist-artwork-fetches/{id}");
        assert_eq!(get(&app, &token, &receipt).await, queued);
        assert_eq!(
            call(&app, "POST", &fetch, Value::Null, Some(&token)).await,
            (StatusCode::ACCEPTED, queued.clone())
        );
        for path in [
            format!("/api/v1/artwork-fetches/{id}"),
            format!("/api/v1/release-artwork-fetches/{id}"),
        ] {
            assert_eq!(
                call(&app, "GET", &path, Value::Null, Some(&token)).await.0,
                StatusCode::NOT_FOUND
            );
        }
        for (token, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(other), StatusCode::NOT_FOUND),
        ] {
            assert_eq!(
                call(&app, "GET", &receipt, Value::Null, token).await.0,
                expected
            );
        }
        assert!(
            hito.catalog
                .run_next_artwork_job_until(std::future::pending::<()>())
                .await
                .unwrap()
        );
        let ready = get(&app, &token, &receipt).await;
        assert_eq!(ready["status"], "ready");
        assert_eq!(ready["attempts"], 1);
        assert_image(&app, &image, &token).await;
        receipts.push((image, fetch, receipt, ready));
    }
    assert_ne!(receipts[0].3["id"], receipts[1].3["id"]);
    assert_eq!(
        provider.requests().await,
        vec![
            ArtworkRequest {
                entity: ArtworkEntity::Playlist,
                provider_id: provider::PLAYLIST_ID.into(),
            };
            2
        ]
    );
    assert_eq!(saved_state(&app, &token, copies, recording).await, before);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/plugins/{}", installed.id),
            json!({"revision":approved["installation"]["revision"]}),
            Some(&token)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, &token, &availability).await,
        json!({"can_fetch":false})
    );
    for (image, fetch, receipt, ready) in &receipts {
        assert_image(&app, image, &token).await;
        assert_eq!(get(&app, &token, receipt).await, *ready);
        assert_eq!(
            call(&app, "POST", fetch, Value::Null, Some(&token)).await,
            (StatusCode::ACCEPTED, ready.clone())
        );
    }
    assert!(
        !hito
            .catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    assert_eq!(provider.requests().await.len(), 2);
    assert_eq!(saved_state(&app, &token, copies, recording).await, before);
    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for (method, path) in [
        ("GET", &image),
        ("GET", &availability),
        ("POST", &fetch),
        ("GET", &receipts[0].2),
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
                playlist_reference: "https://provider.invalid/an-unrelated-submitted-reference"
                    .into(),
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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), provider::PLAYLIST_COVER);
    assert_ne!(bytes.as_ref(), provider::RECORDING_COVER);
    assert_ne!(bytes.as_ref(), provider::RELEASE_COVER);
}

async fn saved_state(app: &Router, token: &str, copies: [Uuid; 2], recording: &str) -> Value {
    json!({
        "catalog":get(app,token,"/api/v1/recordings").await,
        "metadata":get(app,token,&format!("/api/v1/recordings/{recording}/metadata")).await,
        "playlists":get(app,token,"/api/v1/playlists").await,
        "first":get(app,token,&format!("/api/v1/playlists/{}",copies[0])).await,
        "second":get(app,token,&format!("/api/v1/playlists/{}",copies[1])).await,
        "activity":get(app,token,"/api/v1/activity").await,
        "events":get(app,token,"/api/v1/sync?cursor=0").await,
    })
}
