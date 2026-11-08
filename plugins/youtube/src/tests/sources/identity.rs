use super::{
    Capture, bounded_body, bounded_file, exchange, https, owned_aac, private, representation,
};
use crate::{Config, Service};
use axum::{
    Router,
    http::{Method, StatusCode},
};
use hito_plugin_sdk::{RecordingRequest, SourcePage};
use serde_json::json;
use std::{
    collections::HashSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_representation_after_restart_cannot_reuse_an_old_source() {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = executable(directory.path());
    let first_url = cdn.url();
    recording(directory.path(), "HitoTone001", original.len(), &first_url);
    let dispatch = Capture::default().dispatch();
    let service = Service::with_media_client(
        Config {
            executable: executable.clone(),
        },
        cdn.client(),
    );
    let first = source(
        &service.router(),
        &dispatch,
        "HitoTone001",
        directory.path(),
    )
    .await;
    let response = exchange(
        &service.router(),
        &dispatch,
        Method::GET,
        &audio(&first),
        None,
        &[],
    )
    .await;
    representation(&response, StatusCode::OK, original.len(), None);
    assert_eq!(bounded_body(response).await.as_ref(), original.as_slice());
    shutdown(&service).await;
    drop(service);

    let changed_url = first_url.replace("lmt=1730000000000000", "lmt=1730000000000001");
    assert_ne!(first_url, changed_url);
    recording(
        directory.path(),
        "HitoTone001",
        original.len(),
        &changed_url,
    );
    let restarted = Service::with_media_client(Config { executable }, cdn.client());
    let app = restarted.router();
    let changed = source(&app, &dispatch, "HitoTone001", directory.path()).await;
    assert_ne!(
        first, changed,
        "A changed representation reused an old source identity"
    );
    let received = cdn.received().len();
    let response = exchange(&app, &dispatch, Method::GET, &audio(&first), None, &[]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    private(&bounded_body(response).await, directory.path());
    assert_eq!(
        cdn.received().len(),
        received,
        "An old source reached a new representation upstream"
    );
    let response = exchange(&app, &dispatch, Method::GET, &audio(&changed), None, &[]).await;
    representation(&response, StatusCode::OK, original.len(), None);
    assert_eq!(bounded_body(response).await.as_ref(), original.as_slice());
    assert_eq!(calls(directory.path()).len(), 2);
    shutdown(&restarted).await;
    cdn.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_source_cache_reextracts_an_evicted_representation_for_audio() {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = executable(directory.path());
    let signed = cdn.url();
    let dispatch = Capture::default().dispatch();
    let service = Service::with_media_client(Config { executable }, cdn.client());
    let app = service.router();
    let mut sources = Vec::new();
    for index in 0..129 {
        let owner = format!("Hito{index:07}");
        recording(directory.path(), &owner, original.len(), &signed);
        sources.push(source(&app, &dispatch, &owner, directory.path()).await);
    }
    assert_eq!(sources.iter().collect::<HashSet<_>>().len(), 129);
    assert_eq!(calls(directory.path()).len(), 129);
    for _ in 0..2 {
        let response = exchange(
            &app,
            &dispatch,
            Method::GET,
            &audio(&sources[0]),
            Some("bytes=16-47"),
            &[],
        )
        .await;
        representation(
            &response,
            StatusCode::PARTIAL_CONTENT,
            32,
            Some(&format!("bytes 16-47/{}", original.len())),
        );
        assert_eq!(bounded_body(response).await.as_ref(), &original[16..48]);
        let calls = calls(directory.path());
        assert_eq!(
            calls.len(),
            130,
            "The evicted source must re-extract once without a Sources request"
        );
        assert_eq!(calls.last().unwrap(), "Hito0000000");
    }
    shutdown(&service).await;
    cdn.shutdown().await;
}

pub(super) fn executable(directory: &Path) -> PathBuf {
    fs::create_dir(directory.join("recordings")).unwrap();
    let executable = directory.join("owned-canonical-extractor");
    fs::write(
        &executable,
        include_str!("../../fixtures/recording-cache.sh"),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    executable
}

pub(super) fn recording(directory: &Path, owner: &str, length: usize, url: &str) {
    assert_eq!(owner.len(), 11);
    assert!(
        owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    );
    fs::write(
        directory.join("recordings").join(format!("{owner}.json")),
        json!({
            "id":owner, "title":"Owned AAC tone", "artist":"Hito test performer", "duration":0.25,
            "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2", "vcodec":"none",
            "protocol":"https", "filesize":length, "url":url, "is_live":false, "live_status":"not_live"
        })
        .to_string(),
    )
    .unwrap();
}

pub(super) async fn source(
    app: &Router,
    dispatch: &tracing::Dispatch,
    owner: &str,
    directory: &Path,
) -> String {
    let payload = serde_json::to_vec(&RecordingRequest {
        recording_id: owner.into(),
    })
    .unwrap();
    let response = exchange(app, dispatch, Method::POST, "/v1/sources", None, &payload).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "A valid recording could not resolve a source"
    );
    let bytes = bounded_body(response).await;
    private(&bytes, directory);
    let page: SourcePage = serde_json::from_slice(&bytes).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].ready);
    page.items.into_iter().next().unwrap().id
}

pub(super) fn audio(source: &str) -> String {
    let mut url = reqwest::Url::parse("http://owned.invalid/v1/audio").unwrap();
    url.query_pairs_mut().append_pair("source_id", source);
    format!("{}?{}", url.path(), url.query().unwrap())
}

pub(super) fn calls(directory: &Path) -> Vec<String> {
    String::from_utf8(bounded_file(&directory.join("calls")))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

pub(super) async fn shutdown(service: &Service) {
    tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .await
        .unwrap()
        .unwrap();
}
