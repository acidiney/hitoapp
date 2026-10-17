use futures_util::StreamExt;
use hito_plugin_youtube::{Config, Service};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

pub(super) struct Fixture {
    directory: tempfile::TempDir,
    service: Service,
    capture: Capture,
    dispatch: tracing::Dispatch,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<io::Result<()>>,
    client: reqwest::Client,
    base: String,
}

impl Fixture {
    pub(super) async fn start(directory: tempfile::TempDir) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let executable = directory.path().join("owned-extractor");
        fs::write(&executable, include_str!("../fixtures/metadata.sh"))
            .and_then(|()| fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)))
            .unwrap_or_else(|_| panic!("metadata executable setup failed"));
        let capture = Capture::default();
        let dispatch = capture.dispatch();
        let service = Service::new(Config { executable });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|_| panic!("metadata listener setup failed"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|_| panic!("metadata listener address failed"));
        let request_dispatch = dispatch.clone();
        let app = service.router().layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let dispatch = request_dispatch.clone();
                async move { next.run(request).with_subscriber(dispatch).await }
            },
        ));
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(
            async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
            }
            .with_subscriber(dispatch.clone()),
        );
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_else(|_| panic!("metadata client setup failed"));
        Self {
            directory,
            service,
            capture,
            dispatch,
            stop,
            server,
            client,
            base: format!("http://{address}"),
        }
    }

    pub(super) async fn json<T: DeserializeOwned>(
        &self,
        path: &str,
        payload: Option<&Value>,
        phase: &'static str,
    ) -> Result<T, &'static str> {
        let (status, body) = self.response(path, payload, phase).await?;
        if status != reqwest::StatusCode::OK {
            return Err(phase);
        }
        serde_json::from_slice(&body).map_err(|_| phase)
    }

    pub(super) async fn response(
        &self,
        path: &str,
        payload: Option<&Value>,
        phase: &'static str,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), &'static str> {
        let endpoint = format!("{}{path}", self.base);
        let request = match payload {
            Some(payload) => self.client.post(endpoint).json(payload),
            None => self.client.get(endpoint),
        };
        let response = request
            .header("authorization", "Bearer private-bearer-sentinel")
            .header("cookie", "private-cookie-sentinel")
            .send()
            .await
            .map_err(|_| phase)?;
        let status = response.status();
        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| phase)?;
            if chunk.len() > 262_144 - body.len() {
                return Err(phase);
            }
            body.extend_from_slice(&chunk);
        }
        let text = std::str::from_utf8(&body).map_err(|_| phase)?;
        if text.contains("private-")
            || text.contains("https://")
            || text.contains("googlevideo.com")
        {
            return Err("response_privacy");
        }
        Ok((status, body))
    }

    pub(super) async fn finish(self, result: Result<(), &'static str>, expected_calls: &[u8]) {
        let cleanup = tokio::time::timeout(Duration::from_secs(3), self.service.shutdown())
            .with_subscriber(self.dispatch)
            .await;
        let _ = self.stop.send(());
        let server_cleanup = tokio::time::timeout(Duration::from_secs(3), self.server).await;
        assert!(
            matches!(cleanup, Ok(Ok(()))),
            "metadata process cleanup failed"
        );
        assert!(
            matches!(server_cleanup, Ok(Ok(Ok(())))),
            "metadata HTTP cleanup failed"
        );
        let logs = self
            .capture
            .0
            .lock()
            .unwrap_or_else(|_| panic!("metadata logs failed"));
        let logs =
            std::str::from_utf8(&logs).unwrap_or_else(|_| panic!("metadata log encoding failed"));
        for forbidden in [
            "private-",
            "googlevideo.com",
            "Owned",
            "Legacy",
            "HitoMeta",
            "HitoCheck",
            "é",
        ] {
            assert!(
                !logs.contains(forbidden),
                "metadata logs exposed private input"
            );
        }
        assert!(
            !logs.contains(&self.directory.path().to_string_lossy().to_string()),
            "metadata logs exposed private path"
        );
        assert!(
            logs.contains("youtube_recording_complete"),
            "metadata completion log missing"
        );
        if let Err(phase) = result {
            panic!("YouTube metadata boundary failed: {phase}");
        }
        let calls = fs::read(self.directory.path().join("calls"))
            .unwrap_or_else(|_| panic!("metadata invocation accounting failed"));
        assert!(
            calls == expected_calls,
            "metadata extraction accounting failed"
        );
    }
}

pub(super) fn recording(id: &str, title: &str) -> Value {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| panic!("metadata clock setup failed"))
        .as_secs()
        + 900;
    let signed = format!(
        "https://owned.googlevideo.com/videoplayback?expire={expiry}&lmt=1730000000000000&sig=private-signed-sentinel"
    );
    json!({
        "id":id, "title":title, "duration":12.5, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
        "vcodec":"none", "protocol":"https", "filesize":4096, "url":signed,
        "is_live":false, "live_status":"not_live", "channel":"Owned Official channel",
        "channel_is_verified":true, "private_raw_json":"private-raw-json-sentinel"
    })
}

pub(super) fn write(directory: &Path, name: &str, value: &Value) {
    fs::write(directory.join(name), value.to_string())
        .unwrap_or_else(|_| panic!("metadata JSON setup failed"));
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn dispatch(&self) -> tracing::Dispatch {
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::registry()
                .with(tracing_subscriber::filter::filter_fn(|metadata| {
                    metadata.target() == "hito_plugin_youtube"
                        || metadata.target().starts_with("hito_plugin_youtube::")
                }))
                .with(tracing_subscriber::filter::LevelFilter::TRACE)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(move || capture.clone()),
                ),
        )
    }
}

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut capture = self
            .0
            .lock()
            .map_err(|_| io::Error::other("metadata log lock failed"))?;
        if bytes.len() > 1_048_576 - capture.len() {
            return Err(io::Error::other("metadata log bound reached"));
        }
        capture.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
