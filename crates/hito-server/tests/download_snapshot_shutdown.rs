#![cfg(target_os = "linux")]

#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{CoreConfig, DownloadState, HostDeviceId};
use http::{call, get};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

const TEST: &str = "a_download_host_waits_for_its_snapshot_reader_before_shutdown";
const EXPIRED_TEST: &str =
    "a_download_interrupted_during_snapshot_remains_queued_after_lease_expiry";

#[tokio::test]
async fn a_download_host_waits_for_its_snapshot_reader_before_shutdown() {
    isolated(TEST, Duration::from_secs(1), false).await;
}

#[tokio::test]
async fn a_download_interrupted_during_snapshot_remains_queued_after_lease_expiry() {
    isolated(EXPIRED_TEST, Duration::from_secs(33), true).await;
}

async fn isolated(test: &str, hold: Duration, reopen: bool) {
    if std::env::var_os("HITO_DOWNLOAD_SNAPSHOT_CHILD").is_some() {
        child(hold, reopen).await;
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let data = root.join("data");
    let snapshots = root.join("snapshots");
    let gate = root.join("gate");
    for path in [&data, &snapshots, &gate] {
        std::fs::create_dir(path).unwrap();
    }
    std::fs::create_dir(data.join("music")).unwrap();
    let source = root.join("read.c");
    let library = root.join("read.so");
    std::fs::write(&source, include_str!("fixtures/download_snapshot_read.c")).unwrap();
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
        "the owned regular-file read interposer must compile"
    );
    let mut process = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test])
        .env("HITO_DOWNLOAD_SNAPSHOT_CHILD", "1")
        .env("HITO_SNAPSHOT_DATA_ROOT", &data)
        .env("HITO_SNAPSHOT_MANAGED_ROOT", data.join("hito.downloads"))
        .env("HITO_SNAPSHOT_GATE_ROOT", &gate)
        .env("TMPDIR", &snapshots)
        .env("LD_PRELOAD", library)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let limit = Duration::from_secs(if reopen { 75 } else { 20 });
    let result = tokio::time::timeout(limit, process.wait()).await;
    std::fs::write(gate.join("release"), b"").unwrap();
    if result.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(
        result.is_ok_and(|result| result.is_ok_and(|status| status.success())),
        "the isolated host must wait for snapshot reader retirement"
    );
}

async fn child(hold: Duration, reopen: bool) {
    let data = PathBuf::from(std::env::var_os("HITO_SNAPSHOT_DATA_ROOT").unwrap());
    let managed = PathBuf::from(std::env::var_os("HITO_SNAPSHOT_MANAGED_ROOT").unwrap());
    let gate = PathBuf::from(std::env::var_os("HITO_SNAPSHOT_GATE_ROOT").unwrap());
    let snapshots = std::env::temp_dir();
    let original = support::write_wav(&data.join("music/original.wav"), 10);
    let provider = provider::ProviderServer::start().await;
    provider.reply("v1/recordings/detail", json!({
        "id": "owned/snapshot-shutdown", "title": "Owned snapshot fixture", "artist": "Owned fixture"
    })).await;
    provider
        .reply(
            "v1/sources",
            json!({"items": [{
                "id": "owned/snapshot-source", "label": "Original", "ready": true
            }]}),
        )
        .await;
    provider
        .audio(
            StatusCode::OK,
            axum::http::HeaderMap::from_iter([
                (
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                ),
                (
                    "content-length".parse().unwrap(),
                    original.len().to_string().parse().unwrap(),
                ),
            ]),
            original.clone(),
        )
        .await;
    let services = hito_core::open(CoreConfig::new(
        data.join("hito.db"),
        data.join("music"),
        HostDeviceId::new("snapshot-shutdown-host").unwrap(),
    ))
    .await
    .unwrap();
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
            hito_server::router_with_config(services.clone(), config),
            services,
            async {
                let _ = stopped.await;
            },
        )
        .await
        .is_ok()
    });
    let outcome = AssertUnwindSafe(async {
        let (token, recording) = adopt(address, &provider).await;
        let path = format!("/api/v1/recordings/{recording}/download");
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
        let receipt = accepted.json();
        let intent = receipt["intent_id"].as_str().unwrap().to_owned();
        assert!(receipt["recording_id"] == recording);
        wait_until(|| gate.join("entered").exists(), Duration::from_secs(5))
            .await
            .expect("the owned regular managed-file read must enter its gate");
        assert!(gate.join("active").exists());
        assert_eq!(
            std::fs::read_dir(&snapshots).unwrap().count(),
            1,
            "snapshot preparation must own its temporary directory before the held read"
        );
        assert!(owned_reader_fd(&managed, &snapshots));
        let verifying = get(address, &token, &path).await;
        assert!(verifying["status"] == "verifying");
        assert!(verifying["source_id"].is_null());
        assert_eq!(provider.requests("v1/audio").await.len(), 1);
        stop.take().unwrap().send(()).unwrap();
        let completed = wait_until(|| host.is_finished(), hold).await;
        assert!(
            gate.join("active").exists(),
            "the snapshot read must still be held"
        );
        assert!(
            completed.is_err(),
            "host shutdown completed before the owned snapshot reader retired"
        );
        (token, recording, intent)
    })
    .catch_unwind()
    .await;
    if let Some(stop) = stop.take() {
        let _ = stop.send(());
    }
    std::fs::write(gate.join("release"), b"").unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(4), &mut host).await;
    let joined = match joined {
        Ok(result) => matches!(result, Ok(true)),
        Err(_) => {
            host.abort();
            let _ = host.await;
            false
        }
    };
    let clean_at_join = clean(&managed, &snapshots, &gate);
    let retired = wait_until(
        || clean(&managed, &snapshots, &gate),
        Duration::from_secs(4),
    )
    .await
    .is_ok();
    assert!(joined, "the released host must finish joined shutdown");
    assert!(
        retired,
        "owned snapshot readers and temporary files must retire before teardown"
    );
    assert!(std::fs::read(data.join("music/original.wav")).unwrap() == original);
    let (token, recording, intent) = match outcome {
        Ok(context) => context,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        clean_at_join,
        "host completion must follow reader and snapshot cleanup"
    );
    if reopen {
        let reopened = hito_core::open(CoreConfig::new(
            data.join("hito.db"),
            data.join("music"),
            HostDeviceId::new("snapshot-shutdown-host").unwrap(),
        ))
        .await
        .unwrap();
        let actor = reopened.identity.authenticate(&token).await.unwrap();
        let queued = reopened.downloads.status(&actor, &recording).await.unwrap();
        assert!(queued.intent_id == intent);
        assert!(queued.recording_id == recording);
        assert!(
            matches!(queued.status, DownloadState::Queued),
            "an interrupted intent must remain queued when its reader retires after lease expiry"
        );
        assert!(queued.source_id.is_none());
        assert!(queued.error.is_none());
        assert_eq!(provider.requests("v1/audio").await.len(), 1);
    }
}

fn clean(managed: &Path, snapshots: &Path, gate: &Path) -> bool {
    (!gate.join("entered").exists() || gate.join("retired").exists())
        && !gate.join("active").exists()
        && std::fs::read_dir(managed).is_ok_and(|mut files| files.next().is_none())
        && std::fs::read_dir(snapshots).is_ok_and(|mut files| files.next().is_none())
        && !owned_reader_fd(managed, snapshots)
}

fn owned_reader_fd(managed: &Path, snapshots: &Path) -> bool {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            std::fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file())
                && std::fs::read_link(entry.path()).is_ok_and(|target| {
                    target.starts_with(managed) || target.starts_with(snapshots)
                })
        })
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

async fn adopt(address: SocketAddr, provider: &provider::ProviderServer) -> (String, String) {
    let session = post(
        address,
        "/api/v1/setup",
        json!({
            "username": "owner", "password": "owned snapshot shutdown password"
        }),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let service = post(
        address,
        "/api/v1/plugins/services",
        json!({
            "name": "Owned snapshot service", "base_url": provider.url
        }),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installation = post(
        address,
        "/api/v1/plugins",
        json!({
            "service_id": service["id"], "service_revision": service["revision"]
        }),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let recording = post(address, &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()), json!({
        "installation_revision": installation["revision"], "provider_recording_id": "owned/snapshot-shutdown"
    }), Some(token), StatusCode::OK).await;
    (
        token.to_owned(),
        recording["id"].as_str().unwrap().to_owned(),
    )
}

async fn post(
    address: SocketAddr,
    path: &str,
    body: Value,
    token: Option<&str>,
    status: StatusCode,
) -> Value {
    let response = call(address, "POST", path, Some(body), token).await;
    assert_eq!(response.status, status);
    response.json()
}
