use super::support::{admin, app, call};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use image::{ImageEncoder, Rgb, RgbImage};
use serde_json::{Value, json};
use tower::ServiceExt;

const PATH: &str = "/api/v1/account/avatar";
const PASSWORD: &str = "a long avatar test password";

fn png() -> Vec<u8> {
    let picture = RgbImage::from_fn(120, 80, |x, y| Rgb([(x * 2) as u8, (y * 3) as u8, 90]));
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(picture.as_raw(), 120, 80, image::ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    body: Vec<u8>,
    declared: &str,
    token: Option<&str>,
    extra: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", declared)
        .header("x-hito-request", "1");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, bytes.to_vec())
}

async fn upload(app: &Router, token: &str, body: Vec<u8>) -> (StatusCode, Value) {
    let (status, _, bytes) = raw(app, "PUT", PATH, body, "image/png", Some(token), &[]).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn guest(app: &Router, admin: &str, name: &str) -> (String, String) {
    let credentials = json!({"username": name, "password": PASSWORD});
    let (_, user) = call(
        app,
        "POST",
        "/api/v1/users",
        credentials.clone(),
        Some(admin),
    )
    .await;
    let (_, session) = call(app, "POST", "/api/v1/login", credentials, None).await;
    (
        user["id"].as_str().unwrap().to_owned(),
        session["token"].as_str().unwrap().to_owned(),
    )
}

#[tokio::test]
async fn an_account_uploads_replaces_reads_and_removes_its_photo() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    assert_eq!(me["avatar_version"], Value::Null);
    let id = me["id"].as_str().unwrap().to_owned();
    let (status, updated) = upload(&app, &token, png()).await;
    assert_eq!(status, StatusCode::OK);
    let version = updated["avatar_version"].as_str().unwrap().to_owned();
    let (_, me) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    assert_eq!(me["avatar_version"], version.as_str());

    let path = format!("/api/v1/accounts/{id}/avatar");
    let (status, headers, bytes) = raw(
        &app,
        "GET",
        &path,
        Vec::new(),
        "text/plain",
        Some(&token),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/jpeg");
    assert_eq!(headers["cache-control"], "private, no-cache");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["etag"], format!("\"{version}\"").as_str());
    assert_eq!(&bytes[..2], [0xff, 0xd8]);

    let (status, headers, bytes) = raw(
        &app,
        "GET",
        &path,
        Vec::new(),
        "text/plain",
        Some(&token),
        &[("if-none-match", &format!("\"{version}\""))],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(bytes.is_empty());
    assert_eq!(headers["etag"], format!("\"{version}\"").as_str());
    let (status, _, _) = raw(
        &app,
        "GET",
        &path,
        Vec::new(),
        "text/plain",
        Some(&token),
        &[("if-none-match", "\"stale\"")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, removed) = call(&app, "DELETE", PATH, Value::Null, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["avatar_version"], Value::Null);
    let (status, _, _) = raw(
        &app,
        "GET",
        &path,
        Vec::new(),
        "text/plain",
        Some(&token),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn photos_need_a_session_and_are_private_to_the_owner_and_administrators() {
    let (_dir, app) = app().await;
    let admin_token = admin(&app).await;
    let (guest_id, guest_token) = guest(&app, &admin_token, "guest").await;
    let (_, other_token) = guest(&app, &admin_token, "other").await;
    assert_eq!(upload(&app, &guest_token, png()).await.0, StatusCode::OK);
    let path = format!("/api/v1/accounts/{guest_id}/avatar");
    let get = |token: Option<&'static str>| {
        let app = app.clone();
        let path = path.clone();
        async move {
            raw(&app, "GET", &path, Vec::new(), "text/plain", token, &[])
                .await
                .0
        }
    };
    assert_eq!(get(None).await, StatusCode::UNAUTHORIZED);
    let status = |token: &str| {
        let app = app.clone();
        let path = path.clone();
        let token = token.to_owned();
        async move {
            raw(
                &app,
                "GET",
                &path,
                Vec::new(),
                "text/plain",
                Some(&token),
                &[],
            )
            .await
            .0
        }
    };
    assert_eq!(status(&guest_token).await, StatusCode::OK);
    assert_eq!(status(&admin_token).await, StatusCode::OK);
    assert_eq!(status(&other_token).await, StatusCode::NOT_FOUND);
    assert_eq!(
        raw(&app, "PUT", PATH, png(), "image/png", None, &[])
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "DELETE", PATH, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, accounts) = call(
        &app,
        "GET",
        "/api/v1/users",
        Value::Null,
        Some(&admin_token),
    )
    .await;
    let listed = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|account| account["username"] == "guest")
        .unwrap();
    assert!(listed["avatar_version"].is_string());
}

#[tokio::test]
async fn the_type_comes_from_the_bytes_and_bad_uploads_change_nothing() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let (status, _) = upload(&app, &token, png()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, before) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    for body in [
        b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00;".to_vec(),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        Vec::new(),
    ] {
        assert_eq!(
            upload(&app, &token, body).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (_, after) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
    assert_eq!(before["avatar_version"], after["avatar_version"]);
    let (status, _, _) = raw(&app, "PUT", PATH, png(), "text/plain", Some(&token), &[]).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn uploads_over_two_mebibytes_are_refused_while_smaller_ones_pass_the_transport_limit() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let mut big = png();
    big.resize(2 * 1024 * 1024 + 1, 0);
    assert_eq!(
        upload(&app, &token, big).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut padded = png();
    padded.resize(1_500_000, 0);
    let status = upload(&app, &token, padded).await.0;
    assert_ne!(status, StatusCode::PAYLOAD_TOO_LARGE);
}
