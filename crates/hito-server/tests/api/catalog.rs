use super::support::{admin, call, services, write_wav};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn scanned_audio_is_searchable_seekable_and_private() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = write_wav(&music.join("Deterministic fixture.wav"));
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let admin = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&admin)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    let scan = format!("/api/v1/libraries/{library}/scan");
    let (status, job) = call(&app, "POST", &scan, Value::Null, Some(&admin)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(hito.storage.run_next_job().await.unwrap());
    let (_, job) = call(
        &app,
        "GET",
        &format!("/api/v1/jobs/{}", job[0]["id"].as_str().unwrap()),
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(job["state"], "complete");
    let (_, tracks) = call(
        &app,
        "GET",
        "/api/v1/recordings?q=Deterministic",
        Value::Null,
        Some(&admin),
    )
    .await;
    assert_eq!(tracks["total"], 1);
    assert_eq!(tracks["items"][0]["title"], "Deterministic fixture");
    let id = tracks["items"][0]["id"].as_str().unwrap();
    let (_, sources) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{id}/sources"),
        Value::Null,
        Some(&admin),
    )
    .await;
    let source = sources[0]["id"].as_str().unwrap();
    assert!(!sources.to_string().contains(music.to_str().unwrap()));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/sources/{source}/audio"))
                .header("authorization", format!("Bearer {admin}"))
                .header("range", "bytes=44-63")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers()["content-range"],
        format!("bytes 44-63/{}", audio.len())
    );
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        &audio[44..64]
    );
    let (_, bob) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username":"bob","password":"bob private test password"}),
        Some(&admin),
    )
    .await;
    let _ = bob;
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username":"bob","password":"bob private test password"}),
        None,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/recordings", Value::Null, Some(token))
            .await
            .1["total"],
        0
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/recordings/{id}/sources"),
            Value::Null,
            Some(token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/sources/{source}/audio"),
            Value::Null,
            Some(token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn owned_audio_streams_support_suffix_ranges_and_head_without_reopening_paths() {
    let (_dir, app, token, recording) = super::support::indexed_app().await;
    let (_, sources) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording}/sources"),
        Value::Null,
        Some(&token),
    )
    .await;
    let source = sources[0]["id"].as_str().unwrap();
    for (method, range, status, bytes) in [
        ("GET", "bytes=-20", StatusCode::PARTIAL_CONTENT, 20),
        ("GET", "bytes=-20000", StatusCode::PARTIAL_CONTENT, 16044),
        (
            "GET",
            "bytes=-18446744073709551616",
            StatusCode::PARTIAL_CONTENT,
            16044,
        ),
        (
            "GET",
            "bytes=0-18446744073709551616",
            StatusCode::PARTIAL_CONTENT,
            16044,
        ),
        ("GET", "BYTES=0-2", StatusCode::PARTIAL_CONTENT, 3),
        ("GET", "bytes=-+20000", StatusCode::RANGE_NOT_SATISFIABLE, 0),
        ("GET", "bytes=-0", StatusCode::RANGE_NOT_SATISFIABLE, 0),
        ("GET", "bytes=16040-", StatusCode::PARTIAL_CONTENT, 4),
        ("GET", "bytes=99999-", StatusCode::RANGE_NOT_SATISFIABLE, 0),
        ("GET", "bytes=0-2,4-6", StatusCode::RANGE_NOT_SATISFIABLE, 0),
        ("HEAD", "bytes=0-2", StatusCode::OK, 0),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/v1/sources/{source}/audio"))
                    .header("authorization", format!("Bearer {token}"))
                    .header("range", range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        if method == "HEAD" {
            assert_eq!(response.headers()["content-length"], "16044");
        }
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .len(),
            bytes
        );
    }
}
