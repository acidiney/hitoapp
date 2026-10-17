#![cfg(unix)]

#[path = "fixtures/artwork_http.rs"]
mod http;

use axum::{Json, Router, extract::Request, http::StatusCode, routing::get};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    panic::AssertUnwindSafe,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

const PASSWORD: &str = "owned-podcast-private-password-canary";
const PRIVATE_NAME: &str = "owned-podcast-private-manifest-canary";
const PRIVATE_PATH: &str = "owned-podcast-private-path-canary";

#[tokio::test]
async fn production_trace_logging_keeps_dependency_connection_details_private() {
    let directory = tempfile::Builder::new()
        .prefix(PRIVATE_PATH)
        .tempdir()
        .unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let provider = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = provider.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let manifests = requests.clone();
    let router = Router::new().route(
        "/v1/manifest",
        get(move |request: Request| {
            let manifests = manifests.clone();
            async move {
                assert!(
                    request
                        .headers()
                        .get("x-hito-plugin-protocol")
                        .is_some_and(|value| value == "1")
                );
                assert!(request.headers().get("authorization").is_none());
                manifests.fetch_add(1, Ordering::SeqCst);
                Json(
                    json!({"id": "org.hito.owned-log-privacy", "name": PRIVATE_NAME,
                "protocol_version": 1, "capabilities": ["sources"]}),
                )
            }
        }),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_hito-server"))
        .env_clear()
        .env("HITO_DATABASE", directory.path().join("hito.db"))
        .env("HITO_MUSIC_ROOT", directory.path().join("music"))
        .env("HITO_WEB_DIR", web)
        .env("HITO_LISTEN", "127.0.0.1:0")
        .env("HITO_PUBLIC_ORIGIN", "http://127.0.0.1:8080")
        .env("HITO_DEVICE_ID", "owned-log-privacy-host")
        .env("HITO_LOG_FORMAT", "json")
        .env("RUST_LOG", "trace")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (stop, stopped) = oneshot::channel();
    let mut provider = tokio::spawn(async move {
        axum::serve(provider, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .is_ok()
    });
    let (ready, mut listening) = oneshot::channel();
    let stdout = capture(child.stdout.take().unwrap(), Some(ready));
    let stderr = capture(child.stderr.take().unwrap(), None);
    let outcome = AssertUnwindSafe(async {
        let address = timeout(Duration::from_secs(10), &mut listening)
            .await
            .expect("the owned production process must become ready")
            .expect("the owned process must publish its actual listen address");
        assert!(http::get(address, "", "/api/v1/setup").await["needs_setup"] == true);
        let session = http::call(
            address,
            "POST",
            "/api/v1/setup",
            Some(json!({"username": "owned-private-log-account", "password": PASSWORD})),
            None,
        )
        .await;
        assert!(
            session.status == StatusCode::CREATED,
            "owned first-run setup must succeed"
        );
        let session = session.json();
        let token = session["token"]
            .as_str()
            .expect("owned setup must return a session")
            .to_owned();
        let offered = http::call(
            address,
            "POST",
            "/api/v1/plugins/services",
            Some(json!({"name": "", "base_url": format!("http://localhost:{port}/")})),
            Some(&token),
        )
        .await;
        assert!(
            offered.status == StatusCode::CREATED,
            "the real production process must contact the owned plugin"
        );
        let request_id = offered
            .headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .expect("the owned plugin offer must return its safe HTTP correlation identifier")
            .to_owned();
        assert!(
            uuid::Uuid::parse_str(&request_id).is_ok_and(|id| !id.is_nil()),
            "HTTP workflow correlation must use a nonnil UUID"
        );
        assert!(
            offered.json()["name"] == PRIVATE_NAME,
            "the provider manifest must actually supply its display name"
        );
        assert!(
            requests.load(Ordering::SeqCst) == 1,
            "the owned manifest exchange must happen exactly once"
        );
        (token, request_id)
    })
    .catch_unwind()
    .await;

    let exited = terminate(&mut child).await;
    let _ = stop.send(());
    let provider_joined = match timeout(Duration::from_secs(3), &mut provider).await {
        Ok(result) => matches!(result, Ok(true)),
        Err(_) => {
            provider.abort();
            let _ = provider.await;
            false
        }
    };
    let stdout = collected(stdout).await;
    let stderr = collected(stderr).await;
    assert!(
        exited
            && provider_joined
            && stdout.as_ref().is_some_and(|output| output.valid)
            && stderr.as_ref().is_some_and(|output| output.valid),
        "the production process, provider, and bounded log readers must shut down and join"
    );
    let stdout = stdout.unwrap().bytes;
    let stderr = stderr.unwrap().bytes;
    let (token, request_id) = match outcome {
        Ok(acknowledged) => acknowledged,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    let output = [stdout.as_slice(), stderr.as_slice()].concat();
    let output = std::str::from_utf8(&output).expect("captured production logs must be UTF-8");
    assert!(
        !output.contains("localhost"),
        "dependency connection logs must never reveal the owned hostname sentinel"
    );
    for canary in [
        PASSWORD,
        PRIVATE_NAME,
        PRIVATE_PATH,
        "owned-private-log-account",
        &token,
    ] {
        assert!(
            !output.contains(canary),
            "production logs must not emit private workflow canaries"
        );
    }
    let records: Vec<Value> = std::str::from_utf8(&stdout)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .expect("production stdout must contain structured JSON records")
        })
        .collect();
    for expected in ["server_listening", "setup_completed", "server_stopped"] {
        assert!(
            records
                .iter()
                .any(|record| record["fields"]["event"] == expected),
            "production logs must retain the safe application lifecycle markers"
        );
    }
    assert!(
        records
            .iter()
            .any(|record| record["fields"]["event"] == "http_response"
                && record["fields"]["status"] == 201
                && record["span"]["request_id"] == request_id
                && record["span"]["route"] == "/api/v1/plugins/services"),
        "safe production completion logs must correlate to the actual plugin-offer response header"
    );
    assert!(
        records.iter().any(|record| record["level"] == "DEBUG"
            && record["target"] == "hito_core::plugins::transport"
            && record["fields"]["message"] == "Plugin manifest verified"),
        "trace configuration must still enable safe application debug diagnostics"
    );
    assert!(
        records
            .iter()
            .all(|record| record["target"].as_str().is_some_and(|target| [
                "hito_server",
                "hito_core",
                "hito_playback",
                "hito_plugin_sdk"
            ]
            .iter()
            .any(|prefix| target == *prefix || target.starts_with(&format!("{prefix}::"))))),
        "user trace directives must not enable dependency log targets"
    );
}

struct Captured {
    bytes: Vec<u8>,
    valid: bool,
}

fn capture(
    reader: impl AsyncRead + Unpin + Send + 'static,
    mut ready: Option<oneshot::Sender<SocketAddr>>,
) -> JoinHandle<Captured> {
    tokio::spawn(async move {
        let mut reader = reader;
        let mut captured = Captured {
            bytes: Vec::new(),
            valid: true,
        };
        let mut chunk = [0_u8; 8192];
        let mut scanned = 0;
        loop {
            let count = match reader.read(&mut chunk).await {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => {
                    captured.valid = false;
                    break;
                }
            };
            if captured.bytes.len() + count > 4 * 1024 * 1024 {
                captured.valid = false;
                continue;
            }
            captured.bytes.extend_from_slice(&chunk[..count]);
            while let Some(end) = captured.bytes[scanned..]
                .iter()
                .position(|byte| *byte == b'\n')
            {
                let end = scanned + end;
                if ready.is_some()
                    && let Ok(record) =
                        serde_json::from_slice::<Value>(&captured.bytes[scanned..end])
                    && record["fields"]["event"] == "server_listening"
                    && let Some(address) = record["fields"]["address"]
                        .as_str()
                        .and_then(|value| value.parse().ok())
                    && let Some(sender) = ready.take()
                {
                    let _ = sender.send(address);
                }
                scanned = end + 1;
            }
        }
        captured
    })
}

async fn collected(mut task: JoinHandle<Captured>) -> Option<Captured> {
    match timeout(Duration::from_secs(3), &mut task).await {
        Ok(result) => result.ok(),
        Err(_) => {
            task.abort();
            let _ = task.await;
            None
        }
    }
}

async fn terminate(child: &mut Child) -> bool {
    let signalled = match child.try_wait() {
        Ok(Some(status)) => return status.success(),
        Ok(None) => Command::new("sh")
            .args(["-c", "kill -TERM \"$1\"", "hito-log-test"])
            .arg(child.id().unwrap().to_string())
            .status()
            .await
            .is_ok_and(|status| status.success()),
        Err(_) => false,
    };
    match timeout(Duration::from_secs(5), child.wait()).await {
        Ok(result) => signalled && result.is_ok_and(|status| status.success()),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            false
        }
    }
}
