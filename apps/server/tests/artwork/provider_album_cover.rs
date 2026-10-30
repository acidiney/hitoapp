use hito_core::test_support::collection_artwork as provider;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use hito_core::{ImportProviderPlaylist, Services};
use serde_json::{Value, json};
use support::{admin, call};
use uuid::Uuid;

async fn get(app: &axum::Router, token: &str, path: &str) -> (StatusCode, Value) {
    call(app, "GET", path, Value::Null, Some(token)).await
}

#[tokio::test]
async fn reading_an_imported_album_cover_fetches_the_provider_cover_and_honours_the_choice() {
    let directory = tempfile::tempdir().unwrap();
    let hito: Services = support::services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let actor = hito.identity.authenticate(&token).await.unwrap();
    let server = provider::ArtworkServer::start().await;
    let installed = server.connect(&hito, &actor).await;
    hito.collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
            },
        )
        .await
        .unwrap();
    let albums = get(&app, &token, "/api/v1/releases").await.1;
    let release = albums["items"][0]["id"].as_str().unwrap();
    let image = format!("/api/v1/releases/{release}/artwork");
    let availability = format!("{image}/availability");

    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/metadata/automation",
        json!({"automatic_artwork": false}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(get(&app, &token, &image).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, &token, &availability).await.1,
        json!({"can_fetch": false})
    );
    assert!(
        !hito
            .catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );

    call(
        &app,
        "PUT",
        "/api/v1/metadata/automation",
        json!({"automatic_artwork": true}),
        Some(&token),
    )
    .await;
    assert_eq!(get(&app, &token, &image).await.0, StatusCode::NOT_FOUND);
    assert!(
        hito.catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    assert_eq!(server.requests().await.len(), 1);
    assert_eq!(get(&app, &token, &image).await.0, StatusCode::OK);
}
