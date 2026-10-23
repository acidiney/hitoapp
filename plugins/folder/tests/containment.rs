mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_folder::Folder;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn moving_files_and_roots_preserves_ids_and_uncertain_audio_stays_separate() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("private-root");
    std::fs::create_dir(&root).unwrap();
    support::write_wav(&root.join("private-first.wav"), Some("Same tags"), 440);
    support::write_wav(&root.join("private-second.wav"), Some("Same tags"), 441);
    support::write_wav(&root.join("private-untagged.wav"), None, 442);
    std::fs::copy(root.join("private-first.wav"), root.join("duplicate.wav")).unwrap();
    let first = Folder::scan(&root).unwrap().router();
    let before = support::search(&first, "same", None, 20).await;
    assert_eq!(before.items.len(), 2);
    assert_ne!(before.items[0].id, before.items[1].id);
    let untagged = support::search(&first, "untagged", None, 20).await;
    assert_eq!(untagged.items.len(), 1);
    assert!(untagged.items[0].title.starts_with("Untagged audio "));
    assert!(!untagged.items[0].title.contains("private"));
    std::fs::rename(root.join("private-first.wav"), root.join("renamed.wav")).unwrap();
    let moved = directory.path().join("moved-root");
    std::fs::rename(&root, &moved).unwrap();
    let after = support::search(&Folder::scan(&moved).unwrap().router(), "same", None, 20).await;
    assert_eq!(
        before.items.iter().map(|item| &item.id).collect::<Vec<_>>(),
        after.items.iter().map(|item| &item.id).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_special_files_traversal_and_replacements_never_serve_private_files() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    support::write_wav(
        &outside.path().join("outside.wav"),
        Some("Private outside"),
        440,
    );
    symlink(
        outside.path().join("outside.wav"),
        directory.path().join("linked.wav"),
    )
    .unwrap();
    symlink(outside.path(), directory.path().join("linked-directory")).unwrap();
    symlink("/dev/zero", directory.path().join("special.wav")).unwrap();
    let pipe = std::ffi::CString::new(
        directory
            .path()
            .join("named-pipe.wav")
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    let selected = directory.path().join("selected.wav");
    support::write_wav(&selected, Some("Public demo"), 442);
    let app = Folder::scan(directory.path()).unwrap().router();
    assert!(
        support::search(&app, "private", None, 20)
            .await
            .items
            .is_empty()
    );
    let page = support::search(&app, "demo", None, 20).await;
    let recording = &page.items[0];
    let uri = format!("/v1/audio?source_id=source-{}", recording.id);
    std::fs::remove_file(&selected).unwrap();
    symlink(outside.path().join("outside.wav"), &selected).unwrap();
    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = support::post(
        &app,
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    )
    .await;
    let sources: hito_plugin_sdk::SourcePage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(!sources.items[0].ready);
    for id in [
        "../outside.wav",
        "%2fetc%2fpasswd",
        "source-../../outside.wav",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/v1/audio?source_id={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    std::fs::remove_file(&selected).unwrap();
    support::write_wav(&selected, Some("Private replacement"), 443);
    let response = app
        .clone()
        .oneshot(Request::get(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(Folder::scan(&directory.path().join("linked-directory")).is_err());
}

#[tokio::test]
async fn malformed_unknown_and_oversized_requests_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let app = Folder::scan(directory.path()).unwrap().router();
    for body in [
        serde_json::json!({"query":"demo","limit":51}),
        serde_json::json!({"query":"demo","limit":20,"unexpected":"value"}),
        serde_json::json!({"query":"demo\nprivate","limit":1}),
        serde_json::json!({"query":"x".repeat(513),"limit":1}),
    ] {
        assert!(
            !support::post(&app, "/v1/search", body)
                .await
                .status()
                .is_success()
        );
    }
    let response = support::post(
        &app,
        "/v1/search",
        serde_json::json!({"query":"x".repeat(5000),"limit":1}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response = support::post(
        &app,
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":"unknown"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
