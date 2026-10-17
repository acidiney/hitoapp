#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{CoreConfig, HostDeviceId, Services};
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use uuid::Uuid;

const HOST: &str = "download-runtime-host";

#[tokio::test]
async fn an_explicit_download_survives_restart_and_removal_preserves_owned_library_state() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let original = support::write_wav(&music.join("original.wav"), 23);
    let audio = support::write_wav(&directory.path().join("provider.wav"), 10);
    let provider = provider::ProviderServer::start().await;
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
                    audio.len().to_string().parse().unwrap(),
                ),
            ]),
            audio.clone(),
        )
        .await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(exercise(directory.path(), &provider, &audio, &mut hosts))
        .catch_unwind()
        .await;
    for host in &mut hosts {
        host.request_shutdown();
        host.join().await;
    }
    assert!(
        std::fs::read(music.join("original.wav")).unwrap() == original,
        "download work must preserve the user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(
    directory: &Path,
    provider: &provider::ProviderServer,
    audio: &[u8],
    hosts: &mut Vec<Host>,
) {
    hosts.push(Host::start(open(directory).await).await);
    let address = hosts[0].address;
    let response = call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({"username": "owner", "password": "owned download runtime password"})),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    let recording = adopt_recording(address, token, provider).await;
    let playlist = Uuid::new_v4();
    let entry = Uuid::new_v4();
    retain_private_state(address, token, &recording, playlist, entry).await;
    let download_path = format!("/api/v1/recordings/{recording}/download");
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let sources = get(address, token, &sources_path).await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["ready"] == true);
    let provider_source = sources[0]["id"].as_str().unwrap();
    let before = saved_state(address, token, &recording, playlist).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["playlist"]["entries"].as_array().unwrap().len(), 1);
    assert!(before["playlist"]["entries"][0]["entry_id"] == entry.to_string());
    assert!(before["metadata"]["title"]["personal"] == "My retained download title");
    assert!(before["activity"]["history"][0]["position_ms"] == 700);
    assert!(before["activity"]["history"][0]["speed"] == 1.25);
    assert_eq!(provider.requests("v1/audio").await.len(), 0);

    let request_id = Uuid::new_v4();
    let response = call(
        address,
        "POST",
        &download_path,
        Some(json!({"request_id": request_id})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let accepted = response.json();
    assert!(accepted["request_id"] == request_id.to_string());
    assert!(accepted["recording_id"] == recording);
    let intent = accepted["intent_id"].as_str().unwrap().to_owned();
    Uuid::parse_str(&intent).unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let status = get(address, token, &download_path).await;
            if status["status"] == "ready" {
                break status;
            }
            assert!(
                matches!(
                    status["status"].as_str(),
                    Some("queued" | "running" | "verifying")
                ),
                "download state={}, error={}, received={}, total={}",
                status["status"],
                status["error"],
                status["bytes_received"],
                status["total_bytes"]
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the joined host worker must finish the explicit download");
    assert!(ready["recording_id"] == recording);
    assert!(ready["intent_id"] == intent);
    let source = ready["source_id"].as_str().unwrap().to_owned();
    Uuid::parse_str(&source).unwrap();
    assert!(source != provider_source);
    let transfer_count = provider.requests("v1/audio").await.len();
    assert!(
        transfer_count > 0,
        "Download must receive owned provider bytes"
    );

    hosts[0].request_shutdown();
    hosts[0].join().await;
    provider.offline().await;
    hosts.push(Host::start(open(directory).await).await);
    let address = hosts[1].address;
    let retained = get(address, token, &download_path).await;
    assert!(retained["status"] == "ready");
    assert!(retained["intent_id"] == intent);
    assert!(retained["source_id"] == source);
    let sources = get(address, token, &sources_path).await;
    let managed = sources
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["id"] == source)
        .expect("the retained download must be a normal playback candidate");
    assert!(managed["ready"] == true);
    assert!(managed["device_id"] == HOST);
    assert!(managed["recording_id"] == recording);
    assert!(
        sources[0]["id"] == source,
        "ready local audio must precede the provider"
    );
    let audio_path = format!("/api/v1/sources/{source}/audio");
    let response = call(address, "GET", &audio_path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert_eq!(response.body.len(), audio.len());
    assert!(
        response.body == audio,
        "reopened playback must return the complete owned bytes"
    );
    assert_range(address, token, &audio_path, audio).await;
    assert_eq!(provider.requests("v1/audio").await.len(), transfer_count);
    assert!(saved_state(address, token, &recording, playlist).await == before);

    let response = call(
        address,
        "DELETE",
        &format!("/api/v1/downloads/{intent}"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(address, "GET", &audio_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let sources = get(address, token, &sources_path).await;
    assert!(
        sources
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != source)
    );
    assert!(saved_state(address, token, &recording, playlist).await == before);
    assert_eq!(provider.requests("v1/audio").await.len(), transfer_count);
}

async fn adopt_recording(
    address: SocketAddr,
    token: &str,
    provider: &provider::ProviderServer,
) -> String {
    provider
        .reply(
            "v1/recordings/detail",
            json!({"id": "recording/owned-download", "title": "Owned download", "artist": "Owned fixture", "duration_ms": 1000}),
        )
        .await;
    provider
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/private-owned-download", "label": "Original", "ready": true}]}),
        )
        .await;
    let response = call(
        address,
        "POST",
        "/api/v1/plugins/services",
        Some(json!({"name": "Owned download service", "base_url": provider.url})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let response = call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(json!({"service_id": service["id"], "service_revision": service["revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let installation = response.json();
    let response = call(
        address,
        "POST",
        &format!(
            "/api/v1/plugins/{}/recordings",
            installation["id"].as_str().unwrap()
        ),
        Some(json!({"installation_revision": installation["revision"], "provider_recording_id": "recording/owned-download"})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    response.json()["id"].as_str().unwrap().to_owned()
}

async fn retain_private_state(
    address: SocketAddr,
    token: &str,
    recording: &str,
    playlist: Uuid,
    entry: Uuid,
) {
    let actions = [
        json!({"kind": "playlist_create", "playlist_id": playlist, "name": "My retained playlist"}),
        json!({"kind": "entry_insert", "playlist_id": playlist, "entry_id": entry, "recording_id": recording, "position": 0}),
        json!({"kind": "recording_title_set", "recording_id": recording, "title": "My retained download title"}),
        json!({"kind": "like_set", "recording_id": recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": recording, "position_ms": 700, "speed": 1.25}),
    ];
    let operations: Vec<Value> = actions
        .into_iter()
        .map(|action| json!({"id": Uuid::new_v4(), "action": action}))
        .collect();
    let response = call(
        address,
        "POST",
        "/api/v1/sync",
        Some(json!({"device_id": "download-runtime-client", "operations": operations})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["acknowledged"].as_array().unwrap().len(), 5);
}

async fn saved_state(address: SocketAddr, token: &str, recording: &str, playlist: Uuid) -> Value {
    let mut catalog = get(address, token, "/api/v1/recordings?limit=100").await;
    let mut detail = get(address, token, &format!("/api/v1/playlists/{playlist}")).await;
    let mut activity = get(address, token, "/api/v1/activity").await;
    for rows in [
        &mut catalog["items"],
        &mut detail["entries"],
        &mut activity["history"],
    ] {
        for row in rows.as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("available");
        }
    }
    let metadata = get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/metadata"),
    )
    .await;
    json!({"catalog": catalog, "playlist": detail, "activity": activity, "metadata": metadata})
}

async fn assert_range(address: SocketAddr, token: &str, path: &str, audio: &[u8]) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nRange: bytes=44-63\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let split = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&response[..split]).unwrap();
        assert!(
            headers
                .lines()
                .next()
                .unwrap()
                .starts_with("HTTP/1.1 206 ")
        );
        let range = format!("content-range: bytes 44-63/{}", audio.len());
        assert!(headers.lines().any(|line| line.eq_ignore_ascii_case(&range)));
        assert!(
            headers
                .lines()
                .any(|line| line.eq_ignore_ascii_case("content-length: 20"))
        );
        assert_eq!(response[split + 4..].len(), 20);
        assert!(
            response[split + 4..] == audio[44..64],
            "local seeking must return exact owned bytes"
        );
    })
    .await
    .expect("the retained local range read must finish without its provider");
}

async fn open(directory: &Path) -> Services {
    hito_core::open(CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new(HOST).unwrap(),
    ))
    .await
    .unwrap()
}

struct Host {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Host {
    async fn start(services: Services) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = hito_server::Config {
            origin: format!("http://{address}"),
            ..Default::default()
        };
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            hito_server::serve(
                listener,
                hito_server::router_with_config(services.clone(), config),
                services,
                async {
                    let _ = stopped.await;
                },
            )
            .await
            .unwrap();
        });
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }

    fn request_shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    async fn join(&mut self) {
        let Some(mut task) = self.task.take() else {
            return;
        };
        match tokio::time::timeout(Duration::from_secs(4), &mut task).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                task.abort();
                let _ = task.await;
                panic!("the owned host must finish its joined shutdown");
            }
        }
        assert!(TcpStream::connect(self.address).await.is_err());
    }
}
