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

struct GenreFixture {
    _directory: tempfile::TempDir,
    app: Router,
    admin: String,
    library: String,
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

fn tracks_uri(key: &str, limit: Option<i64>, after: Option<&str>) -> String {
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

async fn genre_fixture() -> GenreFixture {
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
                _directory: directory,
                app,
                admin,
                library,
            }
        }),
    )
    .await
}

async fn track_request(
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

async fn bounded_call(
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

async fn create_member(app: &Router, admin_token: &str) -> (String, String) {
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

fn assert_private_json(content_type: Option<HeaderValue>, cache_control: Option<HeaderValue>) {
    assert_eq!(
        content_type,
        Some(HeaderValue::from_static("application/json"))
    );
    assert_eq!(
        cache_control,
        Some(HeaderValue::from_static("private, no-store"))
    );
}

#[tokio::test]
async fn genre_track_reads_use_the_present_account_and_obey_lock_and_revocation() {
    let fixture = genre_fixture().await;
    let uri = tracks_uri("fado", Some(1), None);
    let (status, _, _, _) = track_request(&fixture.app, &uri, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (_member_id, member_token) = hang_guard(
        "genre member account setup",
        Box::pin(create_member(&fixture.app, &fixture.admin)),
    )
    .await;
    let (status, _, _, _) = track_request(
        &fixture.app,
        &uri,
        Some(&member_token),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, content_type, cache_control, bytes) =
        track_request(&fixture.app, &uri, None, Some(&fixture.admin)).await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let cursor = page["next_after"].as_str().unwrap().to_owned();

    let (status, _) = bounded_call(
        &fixture.app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password":"administrator test password","pin":"1234","idle_minutes":0}),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bounded_call(
        &fixture.app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let after_uri = tracks_uri("fado", Some(1), Some(&cursor));
    let (status, _, _, bytes) =
        track_request(&fixture.app, &after_uri, None, Some(&fixture.admin)).await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["code"],
        "locked"
    );

    let (status, _) = bounded_call(
        &fixture.app,
        "POST",
        "/api/v1/account/unlock",
        json!({"pin":"1234"}),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bounded_call(
        &fixture.app,
        "POST",
        "/api/v1/logout",
        Value::Null,
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _, _) = track_request(
        &fixture.app,
        &after_uri,
        Some(&fixture.admin),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn genre_track_cursors_limits_and_encoded_keys_keep_their_scopes() {
    let fixture = genre_fixture().await;
    let (status, member_token) = hang_guard(
        "genre member access setup",
        Box::pin(async {
            let (member_id, member_token) = create_member(&fixture.app, &fixture.admin).await;
            let access = format!("/api/v1/libraries/{}/members/{member_id}", fixture.library);
            let (status, _) = call(
                &fixture.app,
                "PUT",
                &access,
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            (status, member_token)
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _, first_bytes) = track_request(
        &fixture.app,
        &tracks_uri("FADO", Some(1), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first: Value = serde_json::from_slice(&first_bytes).unwrap();
    let cursor = first["next_after"].as_str().unwrap().to_owned();

    let (status, _, _, default_bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", None, None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let default_page: Value = serde_json::from_slice(&default_bytes).unwrap();
    assert_eq!(default_page["items"].as_array().unwrap().len(), 2);
    assert!(default_page["next_after"].is_null());

    let (status, _, _, lower_bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(0), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let lower_page: Value = serde_json::from_slice(&lower_bytes).unwrap();
    assert_eq!(lower_page["items"].as_array().unwrap().len(), 1);
    assert!(lower_page["next_after"].as_str().is_some());

    let (status, _, _, upper_bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(999), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let upper_page: Value = serde_json::from_slice(&upper_bytes).unwrap();
    assert_eq!(upper_page["items"].as_array().unwrap().len(), 2);
    assert!(upper_page["next_after"].is_null());

    for (key, after, expected) in [
        (
            "fado",
            "not-a-valid-cursor",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("jazz", cursor.as_str(), StatusCode::UNPROCESSABLE_ENTITY),
        ("fado", cursor.as_str(), StatusCode::UNPROCESSABLE_ENTITY),
        ("no such genre", cursor.as_str(), StatusCode::NOT_FOUND),
    ] {
        let bearer = if key == "fado" && after == cursor.as_str() {
            member_token.as_str()
        } else {
            fixture.admin.as_str()
        };
        let (status, _, _, _) = track_request(
            &fixture.app,
            &tracks_uri(key, Some(1), Some(after)),
            Some(bearer),
            None,
        )
        .await;
        assert_eq!(status, expected);
    }

    let encoded_key = tracks_uri("CAFÉ + /?", Some(10), None);
    assert!(
        encoded_key.contains("%2B") && encoded_key.contains("%2F") && encoded_key.contains("%3F")
    );
    let (status, content_type, cache_control, bytes) =
        track_request(&fixture.app, &encoded_key, Some(&fixture.admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let unicode_page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(unicode_page["items"].as_array().unwrap().len(), 1);
    assert!(unicode_page["next_after"].is_null());
}
