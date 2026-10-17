use super::{Capture, bounded_body, exchange, https, owned_aac, representation};
use crate::{Config, Service};
use axum::http::{Method, StatusCode};
use hito_plugin_sdk::{RecordingRequest, SourcePage};
use serde_json::json;
use std::{
    collections::HashSet, fs, net::SocketAddr, os::unix::fs::PermissionsExt, path::PathBuf,
    time::Duration,
};

#[tokio::test(flavor = "current_thread")]
async fn shutdown_closes_the_owned_client_socket_before_returning_with_an_unpolled_body() {
    let directory = tempfile::tempdir().unwrap();
    let original = owned_aac(directory.path());
    let mut cdn = https::Fixture::start(original.clone()).await;
    let executable = directory.path().join("owned-recording-extractor");
    fs::write(&executable, include_str!("../../fixtures/recording.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        directory.path().join("recording.json"),
        json!({
            "id":"HitoTone001", "title":"Owned AAC tone", "artist":"Hito test performer",
            "duration":0.25, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
            "vcodec":"none", "protocol":"https", "filesize":original.len(), "url":cdn.url(),
            "is_live":false, "live_status":"not_live"
        })
        .to_string(),
    )
    .unwrap();
    let dispatch = Capture::default().dispatch();
    let service = Service::with_media_client(Config { executable }, cdn.client());
    let app = service.router();
    let payload = serde_json::to_vec(&RecordingRequest {
        recording_id: "HitoTone001".into(),
    })
    .unwrap();
    let response = exchange(&app, &dispatch, Method::POST, "/v1/sources", None, &payload).await;
    assert_eq!(response.status(), StatusCode::OK);
    let page: SourcePage = serde_json::from_slice(&bounded_body(response).await).unwrap();
    page.validate().unwrap();
    assert_eq!(page.items.len(), 1);
    let mut url = reqwest::Url::parse("http://owned.invalid/v1/audio").unwrap();
    url.query_pairs_mut()
        .append_pair("source_id", &page.items[0].id);
    let audio = format!("{}?{}", url.path(), url.query().unwrap());
    cdn.hold_until_closed(1);
    let response = exchange(&app, &dispatch, Method::GET, &audio, None, &[]).await;
    representation(&response, StatusCode::OK, original.len(), None);
    let body = response.into_body();
    let socket = client_socket(cdn.address());
    assert!(socket_targets().contains(&socket));

    let shutdown = tokio::time::timeout(Duration::from_secs(3), service.shutdown()).await;
    let physically_closed_before_yield = !socket_targets().contains(&socket);

    drop(body);
    cdn.closed(1).await;
    cdn.shutdown().await;
    shutdown.unwrap().unwrap();
    assert!(
        physically_closed_before_yield,
        "Service shutdown returned while its concrete client TCP socket was still open"
    );
}

fn client_socket(server: SocketAddr) -> PathBuf {
    let SocketAddr::V4(server) = server else {
        panic!("The owned CDN must use its explicit IPv4 loopback listener");
    };
    let remote = format!(
        "{:08X}:{:04X}",
        u32::from_ne_bytes(server.ip().octets()),
        server.port()
    );
    let sockets = socket_targets();
    let table = fs::read_to_string("/proc/net/tcp").unwrap();
    let matching: Vec<_> = table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 10 || fields[2] != remote || fields[1] == remote || fields[3] != "01"
            {
                return None;
            }
            let target = PathBuf::from(format!("socket:[{}]", fields[9]));
            sockets.contains(&target).then_some(target)
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "The owned CDN tuple must identify exactly one live client socket inode"
    );
    matching.into_iter().next().unwrap()
}

fn socket_targets() -> HashSet<PathBuf> {
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| {
            entry
                .ok()
                .and_then(|entry| fs::read_link(entry.path()).ok())
        })
        .filter(|target| target.to_string_lossy().starts_with("socket:["))
        .collect()
}
