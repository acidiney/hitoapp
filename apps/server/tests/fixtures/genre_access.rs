use crate::support::{admin, call, services};
use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderValue, Request, StatusCode},
};
use hito_core::test_support::{hang_guard::hang_guard, support::write_wav};
use http_body_util::BodyExt;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tower::ServiceExt;

pub(crate) struct GenreFixture {
    pub(crate) app: Router,
    pub(crate) admin: String,
    pub(crate) library: String,
    _directory: tempfile::TempDir,
}

fn tagged(path: &Path, offset: i16, title: &str, genre: &str) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.to_owned());
    tag.set_artist("Owned genre access artist".into());
    tag.set_album("Owned genre access album".into());
    tag.set_genre(genre.to_owned());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

pub(crate) fn tracks_uri(key: &str, limit: Option<i64>, after: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("key", key);
    if let Some(limit) = limit {
        query.append_pair("limit", &limit.to_string());
    }
    if let Some(after) = after {
        query.append_pair("after", after);
    }
    format!("/api/v1/genres/tracks?{}", query.finish())
}

pub(crate) async fn genre_fixture() -> GenreFixture {
    hang_guard(
        "genre access library and public fixture",
        Box::pin(async {
            let directory = tempfile::tempdir().unwrap();
            let music = directory.path().join("music");
            std::fs::create_dir(&music).unwrap();
            for (index, (name, title, genre)) in [
                ("fado-a.wav", "Owned Fado A", "Fado"),
                ("fado-b.wav", "Owned Fado B", "Fado"),
                ("jazz.wav", "Owned Jazz", "Jazz"),
                ("unicode.wav", "Owned Unicode", "Café + /?"),
            ]
            .into_iter()
            .enumerate()
            {
                tagged(&music.join(name), index as i16 + 1, title, genre);
            }

            let hito = services(directory.path(), &music).await;
            let app = hito_server::router(hito.clone());
            let admin = admin(&app).await;
            let (status, libraries) =
                call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&admin)).await;
            assert_eq!(status, StatusCode::OK);
            let library = libraries[0]["id"].as_str().unwrap().to_owned();
            let (status, _) = call(
                &app,
                "POST",
                &format!("/api/v1/libraries/{library}/scan"),
                Value::Null,
                Some(&admin),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED);
            assert!(hito.storage.run_next_job().await.unwrap());

            let (status, genres) =
                call(&app, "GET", "/api/v1/genres", Value::Null, Some(&admin)).await;
            assert_eq!(status, StatusCode::OK);
            let genres = genres.as_array().unwrap();
            assert!(genres.iter().any(|genre| genre["key"] == "fado"));
            assert!(genres.iter().any(|genre| genre["key"] == "jazz"));
            assert!(genres.iter().any(|genre| genre["key"] == "café + /?"));
            let (status, recordings) = call(
                &app,
                "GET",
                "/api/v1/recordings?limit=200",
                Value::Null,
                Some(&admin),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(recordings["items"].as_array().unwrap().len(), 4);

            GenreFixture {
                app,
                admin,
                library,
                _directory: directory,
            }
        }),
    )
    .await
}

pub(crate) async fn track_request(
    app: &Router,
    uri: &str,
    bearer: Option<&str>,
    cookie: Option<&str>,
) -> (StatusCode, Option<HeaderValue>, Option<HeaderValue>, Bytes) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut request = Request::builder().uri(uri).header("x-hito-request", "1");
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        if let Some(cookie) = cookie {
            request = request.header("cookie", format!("hito_session={cookie}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let content_type = response.headers().get("content-type").cloned();
        let cache_control = response.headers().get("cache-control").cloned();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, content_type, cache_control, bytes)
    })
    .await
    .expect("genre authorization request and body finish within 5 seconds")
}

pub(crate) async fn bounded_call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    tokio::time::timeout(Duration::from_secs(5), call(app, method, path, body, token))
        .await
        .expect("genre account request and body finish within 5 seconds")
}

pub(crate) async fn create_member(app: &Router, admin_token: &str) -> (String, String) {
    let (status, member) = call(
        app,
        "POST",
        "/api/v1/users",
        json!({"username":"genre-member","password":"another owned long password"}),
        Some(admin_token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let member_id = member["id"].as_str().unwrap().to_owned();
    let (status, session) = call(
        app,
        "POST",
        "/api/v1/login",
        json!({"username":"genre-member","password":"another owned long password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (member_id, session["token"].as_str().unwrap().to_owned())
}

pub(crate) fn assert_private_json(
    content_type: Option<HeaderValue>,
    cache_control: Option<HeaderValue>,
) {
    assert_eq!(
        content_type,
        Some(HeaderValue::from_static("application/json"))
    );
    assert_eq!(
        cache_control,
        Some(HeaderValue::from_static("private, no-store"))
    );
}
