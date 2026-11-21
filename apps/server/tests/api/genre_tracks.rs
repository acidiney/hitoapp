use crate::support::{admin, call, services};
use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use hito_core::test_support::{hang_guard::hang_guard, support::write_wav};
use http_body_util::BodyExt;
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use serde_json::Value;
use std::{collections::HashSet, path::Path, time::Duration};
use tower::ServiceExt;

fn tagged(path: &Path, offset: i16, title: &str) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Genre Paging Artist".into());
    tag.set_album("Genre Paging Album".into());
    tag.set_genre("Fado".into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn genre_tracks_uri(limit: Option<i64>, after: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("key", "FADO");
    if let Some(limit) = limit {
        query.append_pair("limit", &limit.to_string());
    }
    if let Some(after) = after {
        query.append_pair("after", after);
    }
    format!("/api/v1/genres/tracks?{}", query.finish())
}

async fn inertia_genre(app: &Router, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/genres?genre=FADO")
                .header("x-inertia", "true")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn genre_tracks_page(
    app: &Router,
    token: &str,
    limit: Option<i64>,
    after: Option<&str>,
) -> (
    StatusCode,
    Option<axum::http::HeaderValue>,
    Option<axum::http::HeaderValue>,
    Bytes,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(genre_tracks_uri(limit, after))
                    .header("authorization", format!("Bearer {token}"))
                    .header("x-hito-request", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let content_type = response.headers().get("content-type").cloned();
        let cache_control = response.headers().get("cache-control").cloned();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, content_type, cache_control, body)
    })
    .await
    .expect("genre tracks request and body must finish within 5 seconds")
}

#[tokio::test]
async fn genre_tracks_page_beyond_the_detail_preview_without_losing_title_ties() {
    let (_directory, app, token, baseline) = hang_guard(
        "owned genre paging fixture and public baseline",
        Box::pin(async {
            let directory = tempfile::tempdir().unwrap();
            let music = directory.path().join("music");
            std::fs::create_dir(&music).unwrap();
            for index in 0..201 {
                let title = format!("Song {:02}", index % 7);
                let path = music.join(format!("recording-{index:03}.wav"));
                tagged(&path, index as i16 + 1, &title);
            }

            let hito = services(directory.path(), &music).await;
            let app = hito_server::router(hito.clone());
            let token = admin(&app).await;
            let (status, libraries) =
                call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
            assert_eq!(status, StatusCode::OK);
            let library = libraries[0]["id"].as_str().unwrap();
            let (status, _) = call(
                &app,
                "POST",
                &format!("/api/v1/libraries/{library}/scan"),
                Value::Null,
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED);
            assert!(hito.storage.run_next_job().await.unwrap());

            let (status, first) = call(
                &app,
                "GET",
                "/api/v1/recordings?sort=title&limit=200&offset=0",
                Value::Null,
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(first["items"].as_array().unwrap().len(), 200);
            let (status, second) = call(
                &app,
                "GET",
                "/api/v1/recordings?sort=title&limit=200&offset=200",
                Value::Null,
                Some(&token),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(second["items"].as_array().unwrap().len(), 1);
            let baseline: Vec<Value> = first["items"]
                .as_array()
                .unwrap()
                .iter()
                .chain(second["items"].as_array().unwrap())
                .cloned()
                .collect();
            assert_eq!(baseline.len(), 201);
            let mut recording_ids = HashSet::new();
            assert!(baseline.iter().all(|recording| {
                recording_ids.insert(recording["id"].as_str().unwrap().to_owned())
            }));
            assert!(
                baseline
                    .windows(2)
                    .any(|pair| pair[0]["title"] == pair[1]["title"])
            );

            let (status, page) = inertia_genre(&app, &token).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(page["component"], "GenrePage");
            let preview = &page["props"]["genre"];
            assert!(preview["genre"]["key"] == "fado");
            assert_eq!(preview["tracks"].as_array().unwrap().len(), 100);

            (directory, app, token, baseline)
        }),
    )
    .await;

    let mut after = None;
    let mut seen_cursors = HashSet::new();
    let mut seen_recording_ids = HashSet::new();
    let mut observed = 0;
    for (page_index, expected_count) in [100, 100, 1].into_iter().enumerate() {
        let (status, content_type, cache_control, bytes) = genre_tracks_page(
            &app,
            &token,
            (page_index > 0).then_some(100_i64),
            after.as_deref(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            content_type,
            Some(axum::http::HeaderValue::from_static("application/json"))
        );
        assert_eq!(
            cache_control,
            Some(axum::http::HeaderValue::from_static("private, no-store"))
        );
        let page: Value = serde_json::from_slice(&bytes).unwrap();
        let object = page.as_object().unwrap();
        assert_eq!(object.len(), 2);
        assert!(
            object.keys().map(String::as_str).collect::<HashSet<_>>()
                == HashSet::from(["items", "next_after"]),
            "genre tracks response must contain exactly items and next_after"
        );
        let items = page["items"].as_array().unwrap();
        assert_eq!(items.len(), expected_count);
        for item in items {
            assert!(observed < baseline.len());
            assert!(item == &baseline[observed]);
            let id = item["id"].as_str().unwrap().to_owned();
            assert!(seen_recording_ids.insert(id));
            observed += 1;
        }
        let next_after = page["next_after"].as_str().map(str::to_owned);
        if page_index < 2 {
            let cursor = next_after.expect("nonterminal genre pages must include a cursor");
            assert!(!cursor.is_empty());
            assert!(seen_cursors.insert(cursor.clone()));
            after = Some(cursor);
        } else {
            assert!(page["next_after"].is_null());
        }
    }
    assert_eq!(observed, 201);
    assert_eq!(seen_recording_ids.len(), 201);

    let (status, content_type, cache_control, bytes) =
        genre_tracks_page(&app, &token, Some(999), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type,
        Some(axum::http::HeaderValue::from_static("application/json"))
    );
    assert_eq!(
        cache_control,
        Some(axum::http::HeaderValue::from_static("private, no-store"))
    );
    let page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 200);
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&baseline[..200])
            .all(|(actual, expected)| actual == expected)
    );
    let cursor = page["next_after"].as_str().unwrap().to_owned();
    let (status, _, _, bytes) = genre_tracks_page(&app, &token, Some(999), Some(&cursor)).await;
    assert_eq!(status, StatusCode::OK);
    let terminal: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(terminal["items"].as_array().unwrap().len(), 1);
    assert_eq!(terminal["items"][0], baseline[200]);
    assert!(terminal["next_after"].is_null());
}
