mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_folder::Folder;
use hito_plugin_sdk::{Manifest, SearchPage};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn tagged_audio_is_discoverable_through_the_actual_sdk_contract() {
    let directory = tempfile::tempdir().unwrap();
    support::write_wav(
        &directory.path().join("private-filename.wav"),
        Some("Hito demo tone 440 Hz"),
        440,
    );
    let app = Folder::scan(directory.path()).unwrap().router();
    let manifest = app
        .clone()
        .oneshot(Request::get("/v1/manifest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(manifest.status(), StatusCode::OK);
    let bytes = manifest.into_body().collect().await.unwrap().to_bytes();
    let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.capabilities.len(), 2);
    let search = app
        .oneshot(
            Request::post("/v1/search")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"demo","limit":20}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let bytes = search.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&bytes).contains("private-filename"));
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    page.validate(20).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title, "Hito demo tone 440 Hz");
    assert_eq!(page.items[0].artist, "Hito synthetic demo");
    assert_eq!(page.items[0].duration_ms, Some(20_000));
}

#[tokio::test]
async fn search_pages_are_complete_and_cursors_cannot_cross_searches() {
    let directory = tempfile::tempdir().unwrap();
    for index in 0..3 {
        support::write_wav(
            &directory.path().join(format!("private-{index}.wav")),
            Some(&format!("Demo {index}")),
            440 + index,
        );
    }
    let app = Folder::scan(directory.path()).unwrap().router();
    let first = support::search(&app, "demo", None, 1).await;
    assert_eq!(first.items.len(), 1);
    let second = support::search(
        &app,
        "demo",
        Some(
            first
                .next_cursor
                .as_deref()
                .expect("remaining results need a cursor"),
        ),
        1,
    )
    .await;
    assert_ne!(first.items[0].id, second.items[0].id);
    let third = support::search(&app, "demo", second.next_cursor.as_deref(), 1).await;
    assert_ne!(third.items[0].id, second.items[0].id);
    assert!(third.next_cursor.is_none());
    let mismatch = support::post(
        &app,
        "/v1/search",
        serde_json::json!({"query":"different","limit":1,"cursor":first.next_cursor}),
    )
    .await;
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tagged_tracks_share_their_release_but_uncertain_folders_remain_separate() {
    let directory = tempfile::tempdir().unwrap();
    support::write_wav(
        &directory.path().join("first.wav"),
        Some("Release first"),
        440,
    );
    support::write_wav(
        &directory.path().join("second.wav"),
        Some("Release second"),
        441,
    );
    let other = directory.path().join("private-other-release");
    std::fs::create_dir(&other).unwrap();
    support::write_wav(&other.join("third.wav"), Some("Release third"), 442);
    let app = Folder::scan(directory.path()).unwrap().router();
    let page = support::search(&app, "release", None, 20).await;
    let release = |title: &str| {
        page.items
            .iter()
            .find(|item| item.title == title)
            .unwrap()
            .release
            .as_ref()
            .unwrap()
            .id
            .clone()
    };
    assert_eq!(release("Release first"), release("Release second"));
    assert_ne!(release("Release first"), release("Release third"));
    assert!(!release("Release third").contains("private-other-release"));
}

#[tokio::test]
async fn large_tagged_pages_stay_within_the_transport_budget_without_losing_results() {
    let directory = tempfile::tempdir().unwrap();
    for index in 0..40 {
        support::write_tagged_wav(
            &directory.path().join(format!("{index}.wav")),
            Some(&format!("Demo {index} {}", "t".repeat(2000))),
            440 + index,
            &"a".repeat(2000),
            &"b".repeat(2000),
        );
    }
    let app = Folder::scan(directory.path()).unwrap().router();
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    loop {
        let response = support::post(
            &app,
            "/v1/search",
            serde_json::json!({"query":"demo","limit":50,"cursor":cursor}),
        )
        .await;
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.len() <= 262144);
        let page: hito_plugin_sdk::SearchPage = serde_json::from_slice(&bytes).unwrap();
        page.validate(50).unwrap();
        for item in page.items {
            assert!(ids.insert(item.id));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 40);
}
