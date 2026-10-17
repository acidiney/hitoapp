#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

use futures_util::StreamExt;
use hito_plugin_sdk::{Manifest, ProviderRecording, SearchPage, SourcePage};
use hito_plugin_youtube::{Config, Service};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const TITLE: &str = "Owned tune (Live at the test room)";
const CHANNEL_TITLE: &str = "Owned tune (Official Remix)";
const ARTISTS: &str = "Owned performer, Owned guest";
const CHANNEL: &str = "Channel: Owned Official channel";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_music_artists_survive_detail_without_erasing_video_versions() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().unwrap_or_else(|_| panic!("metadata setup failed"));
    let executable = directory.path().join("owned-extractor");
    fs::write(&executable, include_str!("fixtures/metadata.sh"))
        .and_then(|()| fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)))
        .unwrap_or_else(|_| panic!("metadata executable setup failed"));
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| panic!("metadata clock setup failed"))
        .as_secs()
        + 900;
    let signed = format!(
        "https://owned.googlevideo.com/videoplayback?expire={expiry}&lmt=1730000000000000&sig=private-signed-sentinel"
    );
    let common = json!({
        "duration":12.5, "format_id":"140", "ext":"m4a", "acodec":"mp4a.40.2",
        "vcodec":"none", "protocol":"https", "filesize":4096, "url":signed,
        "is_live":false, "live_status":"not_live", "channel":"Owned Official channel",
        "channel_is_verified":true, "private_raw_json":"private-raw-json-sentinel"
    });
    let mut structured = common.clone();
    structured["id"] = json!("HitoMeta001");
    structured["title"] = json!(TITLE);
    structured["track"] = json!("Owned tune");
    structured["artists"] = json!(["Owned performer", "Owned guest"]);
    structured["artist"] = json!("Legacy incomplete performer");
    structured["album"] = json!("Owned live album");
    structured["album_artists"] = json!(["Owned performer"]);
    let mut channel = common;
    channel["id"] = json!("HitoMeta002");
    channel["title"] = json!(CHANNEL_TITLE);
    let search = json!({"entries":[
        {"id":"HitoMeta001","title":TITLE,"channel":"Owned Official channel","duration":12.5},
        {"id":"HitoMeta002","title":CHANNEL_TITLE,"channel":"Owned Official channel","duration":12.5}
    ]});
    for (name, value) in [
        ("search.json", search),
        ("structured.json", structured),
        ("channel.json", channel),
    ] {
        fs::write(directory.path().join(name), value.to_string())
            .unwrap_or_else(|_| panic!("metadata JSON setup failed"));
    }

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
    let result = verify(&client, &format!("http://{address}")).await;
    let cleanup = tokio::time::timeout(Duration::from_secs(3), service.shutdown())
        .with_subscriber(dispatch)
        .await;
    let _ = stop.send(());
    let server_cleanup = tokio::time::timeout(Duration::from_secs(3), server).await;
    assert!(
        matches!(cleanup, Ok(Ok(()))),
        "metadata process cleanup failed"
    );
    assert!(
        matches!(server_cleanup, Ok(Ok(Ok(())))),
        "metadata HTTP cleanup failed"
    );
    let logs = capture
        .0
        .lock()
        .unwrap_or_else(|_| panic!("metadata logs failed"));
    let logs =
        std::str::from_utf8(&logs).unwrap_or_else(|_| panic!("metadata log encoding failed"));
    for forbidden in [
        "private-",
        "googlevideo.com",
        TITLE,
        CHANNEL_TITLE,
        ARTISTS,
        "Owned performer",
        "Owned Official channel",
        "Owned live album",
    ] {
        assert!(
            !logs.contains(forbidden),
            "metadata logs exposed private input"
        );
    }
    assert!(
        !logs.contains(&directory.path().to_string_lossy().to_string()),
        "metadata logs exposed private path"
    );
    assert!(
        logs.contains("youtube_recording_complete"),
        "metadata completion log missing"
    );
    if let Err(phase) = result {
        panic!("YouTube metadata boundary failed: {phase}");
    }
    let calls = fs::read(directory.path().join("calls"))
        .unwrap_or_else(|_| panic!("metadata invocation accounting failed"));
    assert!(
        calls == b"1\n2\n3\n",
        "metadata cache repeated an extraction"
    );
}

async fn verify(client: &reqwest::Client, base: &str) -> Result<(), &'static str> {
    let manifest: Manifest = request(client.get(format!("{base}/v1/manifest")), "manifest").await?;
    manifest.validate().map_err(|_| "manifest")?;
    let page: SearchPage = request(
        client
            .post(format!("{base}/v1/search"))
            .json(&json!({"query":"private-search-query-sentinel","cursor":null,"limit":2})),
        "flat_search",
    )
    .await?;
    page.validate(2).map_err(|_| "flat_search")?;
    if page.items.len() != 2
        || page.next_cursor.is_some()
        || page.items[0].id != "HitoMeta001"
        || page.items[1].id != "HitoMeta002"
        || page
            .items
            .iter()
            .any(|item| item.artist != CHANNEL || item.release.is_some())
        || page.items[0].title != TITLE
        || page.items[1].title != CHANNEL_TITLE
    {
        return Err("flat_search");
    }
    for (id, title, artist, phase) in [
        ("HitoMeta001", TITLE, ARTISTS, "structured_detail"),
        ("HitoMeta002", CHANNEL_TITLE, CHANNEL, "channel_detail"),
    ] {
        let payload = json!({"recording_id":id});
        let detail: ProviderRecording = request(
            client
                .post(format!("{base}/v1/recordings/detail"))
                .json(&payload),
            phase,
        )
        .await?;
        detail.validate().map_err(|_| phase)?;
        if detail.id != id
            || detail.title != title
            || detail.artist != artist
            || detail.duration_ms != Some(12_500)
            || detail.release.is_some()
        {
            return Err(phase);
        }
        let mut source_id = None;
        for _ in 0..2 {
            let sources: SourcePage = request(
                client.post(format!("{base}/v1/sources")).json(&payload),
                "source_identity",
            )
            .await?;
            sources.validate().map_err(|_| "source_identity")?;
            if sources.items.len() != 1
                || !sources.items[0].ready
                || sources.items[0].label != "AAC / M4A"
                || !sources.items[0]
                    .id
                    .starts_with(&format!("youtube:v1:{id}:"))
                || source_id
                    .as_ref()
                    .is_some_and(|previous| previous != &sources.items[0].id)
            {
                return Err("source_identity");
            }
            source_id = Some(sources.items[0].id.clone());
        }
    }
    Ok(())
}

async fn request<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
    phase: &'static str,
) -> Result<T, &'static str> {
    let response = request
        .header("authorization", "Bearer private-bearer-sentinel")
        .header("cookie", "private-cookie-sentinel")
        .send()
        .await
        .map_err(|_| phase)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(phase);
    }
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
    if text.contains("private-") || text.contains("https://") || text.contains("googlevideo.com") {
        return Err("response_privacy");
    }
    serde_json::from_slice(&body).map_err(|_| phase)
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
