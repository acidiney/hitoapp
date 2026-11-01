use hito_core::test_support::artist_images::{FakeArtistImages, Photo};
use hito_core::test_support::covers::COVER_A;
use hito_core::test_support::musicbrainz::FakeMusicBrainz;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use support::{admin, call};
use tower::ServiceExt;

const ARTIST: &str = "0f800000-0000-4000-8000-0000000000c1";

async fn read(
    app: &axum::Router,
    token: Option<&str>,
    path: &str,
) -> (StatusCode, String, Vec<u8>) {
    let mut request = Request::builder().uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cache = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, cache, bytes.to_vec())
}

#[tokio::test]
async fn an_artist_photo_is_private_queued_then_served_and_never_requested_when_artwork_is_off() {
    let musicbrainz = FakeMusicBrainz::start().await;
    let images = FakeArtistImages::start().await;
    musicbrainz.links_of(ARTIST, Some("Q42"));
    images.entity("Q42", Some("Band photo.png"));
    images.photo("Band_photo.png", Photo::Png(COVER_A.to_vec()));
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut config = hito_core::CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("artist-photo-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    config.artist_image_loopback_endpoint = Some(images.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let path = format!("/api/v1/cover-art/artist/{ARTIST}");

    assert_eq!(read(&app, None, &path).await.0, StatusCode::UNAUTHORIZED);
    let (status, _, _) = read(&app, Some(&token), "/api/v1/cover-art/artist/not-an-id").await;
    assert!(status.is_client_error());

    assert_eq!(
        read(&app, Some(&token), &path).await.0,
        StatusCode::ACCEPTED
    );
    assert!(
        hito.catalog
            .run_next_cover_art_lookup_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    let (status, cache, bytes) = read(&app, Some(&token), &path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache, "private, no-store");
    assert_eq!(bytes, COVER_A);

    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/metadata/automation",
        json!({"automatic_artwork": false}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let other = "0f800000-0000-4000-8000-0000000000c2";
    let before = images.requests().len();
    assert_eq!(
        read(
            &app,
            Some(&token),
            &format!("/api/v1/cover-art/artist/{other}")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        !hito
            .catalog
            .run_next_cover_art_lookup_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    assert_eq!(images.requests().len(), before);
}
