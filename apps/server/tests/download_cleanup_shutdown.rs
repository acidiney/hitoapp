#![cfg(target_os = "linux")]

use hito_core::test_support::provider_audio as audio_fixture;
#[path = "fixtures/artwork_http.rs"]
mod http;

use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use futures_util::FutureExt;
use http::{call, get};
use serde_json::json;
use std::{
    io::Write,
    net::SocketAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use uuid::Uuid;

const TEST: &str = "a_download_host_waits_for_removal_directory_sync_before_shutdown";

#[tokio::test]
async fn a_download_host_waits_for_removal_directory_sync_before_shutdown() {
    if std::env::var_os("HITO_DOWNLOAD_CLEANUP_CHILD").is_some() {
        child().await;
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let fixtures = root.join("fixtures");
    let gate = root.join("gate");
    for path in [&fixtures, &gate] {
        std::fs::create_dir(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let source = root.join("sync.c");
    let library = root.join("sync.so");
    std::fs::write(&source, include_str!("fixtures/download_cleanup_sync.c")).unwrap();
    let mut compiler = tokio::process::Command::new("cc")
        .args(["-shared", "-fPIC", "-std=c11", "-o"])
        .arg(&library)
        .arg(&source)
        .arg("-ldl")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let compiled = tokio::time::timeout(Duration::from_secs(10), compiler.wait()).await;
    if compiled.is_err() {
        let _ = compiler.kill().await;
        let _ = compiler.wait().await;
    }
    assert!(
        compiled.is_ok_and(|result| result.is_ok_and(|status| status.success())),
        "the owned directory-sync interposer must compile"
    );
    let mut process = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST])
        .env("HITO_DOWNLOAD_CLEANUP_CHILD", "1")
        .env("HITO_DOWNLOAD_CLEANUP_FIXTURE_ROOT", &fixtures)
        .env("HITO_DOWNLOAD_CLEANUP_GATE_ROOT", &gate)
        .env("TMPDIR", fixtures)
        .env("LD_PRELOAD", library)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(60), process.wait()).await;
    std::fs::write(gate.join("release"), b"").unwrap();
    if result.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(
        result.is_ok_and(|result| result.is_ok_and(|status| status.success())),
        "the isolated host must wait for managed-directory sync retirement"
    );
}

async fn child() {
    let gate = PathBuf::from(std::env::var_os("HITO_DOWNLOAD_CLEANUP_GATE_ROOT").unwrap());
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    let AudioLibrary {
        directory,
        music,
        server: provider,
        hito,
        source,
        original,
        ..
    } = fixture;
    let managed = directory.path().join("hito.downloads");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = hito_server::Config {
        origin: format!("http://{address}"),
        ..Default::default()
    };
    let (stop, stopped) = oneshot::channel();
    let mut stop = Some(stop);
    let mut host = tokio::spawn(async move {
        hito_server::serve(
            listener,
            hito_server::router_with_config(hito.clone(), config),
            hito,
            async {
                let _ = stopped.await;
            },
        )
        .await
        .is_ok()
    });
    let mut removal = None;
    let outcome = AssertUnwindSafe(async {
        let session = call(
            address,
            "POST",
            "/api/v1/login",
            Some(json!({"username": "admin", "password": "indexing test password"})),
            None,
        )
        .await;
        assert_eq!(session.status, StatusCode::OK);
        let token = session.json()["token"].as_str().unwrap().to_owned();
        let path = format!("/api/v1/recordings/{}/download", source.recording_id);
        assert_eq!(provider.requests("v1/audio").await.len(), 0);
        let accepted = call(
            address,
            "POST",
            &path,
            Some(json!({"request_id": Uuid::new_v4()})),
            Some(&token),
        )
        .await;
        assert_eq!(accepted.status, StatusCode::ACCEPTED);
        assert_eq!(accepted.headers["content-type"], "application/json");
        let intent = accepted.json()["intent_id"].as_str().unwrap().to_owned();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let status = get(address, &token, &path).await;
                if status["status"] == "ready" {
                    assert!(status["intent_id"] == intent);
                    break;
                }
                assert!(matches!(
                    status["status"].as_str(),
                    Some("queued" | "running" | "verifying")
                ));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the owned download must become ready before arming sync interception");
        assert_private_copy(&managed, &original);
        assert!(!gate.join("entered").exists());
        let mut selection = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(gate.join("managed-root"))
            .unwrap();
        selection
            .write_all(managed.as_os_str().as_encoded_bytes())
            .unwrap();
        drop(selection);
        std::fs::write(gate.join("enabled"), b"").unwrap();
        removal = Some(tokio::spawn(remove(address, token, intent)));
        wait_until(|| gate.join("entered").exists(), Duration::from_secs(5))
            .await
            .expect("explicit removal must enter the exact owned managed-directory sync");
        assert!(gate.join("active").exists());
        assert!(managed_fd(&managed));
        stop.take().unwrap().send(()).unwrap();
        let completed = wait_until(|| host.is_finished(), Duration::from_secs(6)).await;
        assert!(
            gate.join("active").exists(),
            "the actual managed-directory sync must remain held beyond its operation timeout"
        );
        assert!(
            completed.is_err(),
            "host shutdown completed before owned download-removal sync retired"
        );
    })
    .catch_unwind()
    .await;
    if let Some(stop) = stop.take() {
        let _ = stop.send(());
    }
    std::fs::write(gate.join("release"), b"").unwrap();
    let joined = match tokio::time::timeout(Duration::from_secs(5), &mut host).await {
        Ok(result) => matches!(result, Ok(true)),
        Err(_) => {
            host.abort();
            let _ = host.await;
            false
        }
    };
    let clean_at_join = clean(&managed, &gate);
    let removed = if let Some(mut removal) = removal {
        match tokio::time::timeout(Duration::from_secs(5), &mut removal).await {
            Ok(result) => matches!(result, Ok(StatusCode::NO_CONTENT)),
            Err(_) => {
                removal.abort();
                let _ = removal.await;
                false
            }
        }
    } else {
        true
    };
    let retired = wait_until(|| clean(&managed, &gate), Duration::from_secs(5))
        .await
        .is_ok();
    assert!(joined, "the released host must finish joined shutdown");
    assert!(
        retired,
        "owned directory sync and file descriptors must retire before teardown"
    );
    assert!(std::fs::read(music.join("original.wav")).unwrap() == original);
    assert_eq!(provider.requests("v1/audio").await.len(), 1);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    assert!(
        removed,
        "the accepted removal must retain its successful HTTP response"
    );
    assert!(
        clean_at_join,
        "host completion must follow owned directory-sync retirement"
    );
}

fn assert_private_copy(managed: &Path, original: &[u8]) {
    let root = std::fs::metadata(managed).unwrap();
    assert!(root.is_dir());
    assert_eq!(root.mode() & 0o777, 0o700);
    let files = std::fs::read_dir(managed)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let file = &files[0];
    let name = file.file_name();
    let names = name.to_str().unwrap().split('.').collect::<Vec<_>>();
    assert_eq!(names.len(), 3);
    assert!(Uuid::parse_str(names[0]).is_ok());
    assert!(Uuid::parse_str(names[1]).is_ok());
    assert_eq!(names[2], "media");
    let metadata = std::fs::symlink_metadata(file.path()).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), root.uid());
    assert_eq!(metadata.nlink(), 1);
    assert!(std::fs::read(file.path()).unwrap() == original);
}

fn managed_fd(managed: &Path) -> bool {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            std::fs::read_link(entry.path())
                .is_ok_and(|target| target == managed || target.starts_with(managed))
        })
}

fn clean(managed: &Path, gate: &Path) -> bool {
    (!gate.join("entered").exists() || gate.join("retired").exists())
        && !gate.join("active").exists()
        && (!gate.join("entered").exists()
            || std::fs::read_dir(managed).is_ok_and(|mut entries| entries.next().is_none()))
        && !managed_fd(managed)
}

async fn wait_until(
    predicate: impl Fn() -> bool,
    timeout: Duration,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(timeout, async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

async fn remove(address: SocketAddr, token: String, intent: String) -> StatusCode {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!(
            "DELETE /api/v1/downloads/{intent} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nX-Hito-Request: 1\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let split = response.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
        let head = std::str::from_utf8(&response[..split]).unwrap();
        let status = StatusCode::from_u16(head.lines().next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap()).unwrap();
        if status == StatusCode::NO_CONTENT {
            assert_eq!(response.len(), split + 4);
            assert!(!head.to_ascii_lowercase().contains("content-length:"));
            assert!(!head.to_ascii_lowercase().contains("transfer-encoding:"));
        }
        status
    })
    .await
    .expect("the owned removal HTTP request must finish")
}
