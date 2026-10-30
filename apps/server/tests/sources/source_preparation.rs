#[allow(dead_code)]
#[path = "../api/support.rs"]
mod support;

#[path = "../fixtures/source_preparation.rs"]
mod capable;
#[path = "../fixtures/artwork_http.rs"]
mod http;
#[path = "../fixtures/source_preparation_reconnection.rs"]
mod reconnection;
#[path = "../fixtures/source_preparation_terminal.rs"]
mod terminal;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    panic::AssertUnwindSafe,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{admin, app, call};
use tokio::sync::oneshot;
use uuid::Uuid;

const SERVICE_TOKEN: &str = "owned-preparation-service-token";

struct SourceProvider {
    url: String,
    unexpected_requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl SourceProvider {
    async fn start() -> Self {
        let unexpected_requests = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .fallback(provider)
            .with_state(unexpected_requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            unexpected_requests,
            task,
        }
    }
}

impl Drop for SourceProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn provider(
    State(unexpected_requests): State<Arc<AtomicUsize>>,
    request: Request,
) -> Response {
    let response = match request.uri().path() {
        "/v1/manifest" => json!({
            "id": "org.hito.source-preparation-fixture",
            "name": "Owned source provider",
            "protocol_version": 1,
            "capabilities": ["sources"]
        }),
        "/v1/recordings/detail" => json!({
            "id": "owned-recording",
            "title": "Owned unavailable recording",
            "artist": "Owned artist"
        }),
        "/v1/sources" => json!({
            "items": [{
                "id": "owned-source",
                "label": "Owned unavailable source",
                "ready": false
            }]
        }),
        _ => {
            unexpected_requests.fetch_add(1, Ordering::SeqCst);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    let expected_authorization = format!("Bearer {SERVICE_TOKEN}");
    if request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(expected_authorization.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(response).into_response()
}

#[tokio::test]
async fn source_preparation_requires_the_connections_approved_capability() {
    let provider = SourceProvider::start().await;
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Owned source provider", "base_url": provider.url}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, connection) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({
            "service_id": service["id"],
            "service_revision": service["revision"],
            "bearer_token": SERVICE_TOKEN
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, recording) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/recordings",
            connection["id"].as_str().unwrap()
        ),
        json!({
            "installation_revision": connection["revision"],
            "provider_recording_id": "owned-recording"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recording["available"], false);
    let recording = recording["id"].as_str().unwrap();
    let (status, sources) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording}/sources"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert_eq!(sources[0]["ready"], false);
    let source = sources[0]["id"].as_str().unwrap();
    let (status, response) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{recording}/sources/{source}/preparation"),
        json!({"request_id": "4e470be1-c6b6-4fbd-80f4-0258b72b1db0"}),
        Some(&token),
    )
    .await;
    assert_eq!(provider.unexpected_requests.load(Ordering::SeqCst), 0);
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
}

#[tokio::test]
async fn a_capable_source_is_prepared_once_and_resumes_polling_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let audio = support::write_wav(&directory.path().join("owned-selected-file.wav"));
    let provider = capable::CapableProvider::start(audio.clone()).await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(prepare_with_runtime(
        directory.path(),
        &provider,
        &audio,
        &mut hosts,
    ))
    .catch_unwind()
    .await;
    for host in &mut hosts {
        host.finish().await;
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn a_recreated_connection_cannot_duplicate_an_unresolved_preparation() {
    let directory = tempfile::tempdir().unwrap();
    let audio = support::write_wav(&directory.path().join("owned-unresolved-file.wav"));
    let provider = capable::CapableProvider::start(audio).await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(reconnection::run(directory.path(), &provider, &mut hosts))
        .catch_unwind()
        .await;
    for host in &mut hosts {
        host.finish().await;
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn a_terminal_preparation_allows_reconnection_without_changing_old_receipts() {
    for terminal_ack_after_removal in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let audio = support::write_wav(&directory.path().join("owned-terminal-file.wav"));
        let provider = capable::CapableProvider::start(audio).await;
        let mut hosts = Vec::new();
        let outcome = AssertUnwindSafe(terminal::run(
            directory.path(),
            &provider,
            &mut hosts,
            terminal_ack_after_removal,
        ))
        .catch_unwind()
        .await;
        for host in &mut hosts {
            host.finish().await;
        }
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}

async fn prepare_with_runtime(
    directory: &Path,
    provider: &capable::CapableProvider,
    audio: &[u8],
    hosts: &mut Vec<RuntimeHost>,
) {
    let services = support::services(directory, &directory.join("music")).await;
    hosts.push(RuntimeHost::start(services.clone()).await);
    let address = hosts[0].address;
    let response = http::call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({
            "username": "owner", "password": "owned preparation runtime password"
        })),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins/services",
        Some(json!({
            "name": "Owned preparation provider", "base_url": provider.url
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let response = http::call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(json!({
            "service_id": service["id"], "service_revision": service["revision"],
            "bearer_token": SERVICE_TOKEN
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let connection = response.json();
    let response = http::call(address, "POST", &format!(
        "/api/v1/plugins/{}/recordings", connection["id"].as_str().unwrap()
    ), Some(json!({
        "installation_revision": connection["revision"], "provider_recording_id": capable::RECORDING
    })), Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    let recording = response.json();
    let recording = recording["id"].as_str().unwrap();
    let sources_path = format!("/api/v1/recordings/{recording}/sources");
    let before = http::get(address, token, &sources_path).await;
    assert_eq!(before.as_array().unwrap().len(), 2);
    let source = before
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == false)
        .unwrap();
    let source = source["id"].as_str().unwrap();
    let control = before
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["ready"] == true)
        .unwrap();
    let actor = services.identity.authenticate(token).await.unwrap();
    let control = services
        .playback
        .playback_source(&actor, recording, control["id"].as_str().unwrap())
        .await
        .unwrap();
    control.check().await.unwrap();
    let settings_before = http::get(address, token, "/api/v1/plugins").await;
    let download_path = format!("/api/v1/recordings/{recording}/download");
    assert_eq!(
        http::call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let preparation_path = format!("{sources_path}/{source}/preparation");
    let request = json!({"request_id": "41ba145a-c509-46eb-9d01-e82c1f3a3b82"});
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    assert_eq!(receipt["request_id"], request["request_id"]);
    assert_eq!(receipt["recording_id"], recording);
    assert_eq!(receipt["source_id"], source);
    let operation = receipt["preparation_id"].as_str().unwrap();
    assert!(!Uuid::parse_str(operation).unwrap().is_nil());
    let queued = http::get(address, token, &preparation_path).await;
    assert_eq!(queued["preparation_id"], operation);
    assert!(matches!(
        queued["phase"].as_str(),
        Some("queued" | "submitting")
    ));
    assert_eq!(queued["recording_id"], recording);
    assert_eq!(queued["source_id"], source);
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), receipt);
    let other_request = json!({"request_id": "20e457f2-2212-4531-b580-70692fa6ce12"});
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(other_request.clone()),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let coalesced = response.json();
    assert_eq!(coalesced["request_id"], other_request["request_id"]);
    assert_eq!(coalesced["preparation_id"], operation);
    assert_eq!(coalesced["recording_id"], recording);
    assert_eq!(coalesced["source_id"], source);

    let begin = tokio::time::timeout(Duration::from_secs(3), provider.next_begin())
        .await
        .expect(
            "the real host worker must send one preparation begin for the durable queued intent",
        );
    assert_exchange(&begin, operation, false);
    assert_eq!(provider.counts(), (1, 0, 0));
    begin.accept();
    let poll = tokio::time::timeout(Duration::from_secs(8), provider.next_poll())
        .await
        .expect("the real host worker must poll the acknowledged preparation handle");
    assert_exchange(&poll, operation, true);
    let preparing = http::get(address, token, &preparation_path).await;
    assert_eq!(preparing["phase"], "preparing");
    assert_eq!(preparing["progress_basis_points"], 2500);
    assert_private_status(&preparing);
    hosts[0].request_shutdown();
    drop(poll);
    hosts[0].finish().await;
    drop(services);

    let reopened = support::services(directory, &directory.join("music")).await;
    hosts.push(RuntimeHost::start(reopened).await);
    let address = hosts[1].address;
    let poll = tokio::time::timeout(Duration::from_secs(8), provider.next_poll())
        .await
        .expect("a new host must resume polling the persisted handle without another begin");
    assert_exchange(&poll, operation, true);
    assert_eq!(provider.counts(), (1, 2, 0));
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(request),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), receipt);
    poll.ready();
    let ready = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = http::get(address, token, &preparation_path).await;
            if status["phase"] == "ready" {
                break status;
            }
            assert_eq!(status["phase"], "preparing");
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the host must publish the prepared source after the ready poll response");
    assert_eq!(ready["preparation_id"], operation);
    assert_eq!(ready["recording_id"], recording);
    assert_eq!(ready["source_id"], source);
    assert_private_status(&ready);
    let after = http::get(address, token, &sources_path).await;
    assert_eq!(after.as_array().unwrap().len(), 2);
    for previous in before.as_array().unwrap() {
        let current = after
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["id"] == previous["id"])
            .unwrap();
        assert_eq!(current["recording_id"], recording);
        assert_eq!(current["provider"], previous["provider"]);
        assert_eq!(current["ready"], true);
    }
    let catalog = http::get(address, token, "/api/v1/recordings").await;
    assert_eq!(catalog["total"], 1);
    assert_eq!(catalog["items"][0]["id"], recording);
    assert_eq!(
        http::get(address, token, "/api/v1/plugins").await,
        settings_before
    );
    control.check().await.expect(
        "preparation progress and readiness must preserve existing playback authority generations",
    );
    assert_eq!(
        http::call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let audio_response = http::call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(audio_response.status, StatusCode::OK);
    assert_eq!(audio_response.headers["content-type"], "audio/wav");
    assert_eq!(audio_response.body, audio);
    let response = http::call(
        address,
        "POST",
        &preparation_path,
        Some(other_request),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.json(), coalesced);
    assert_eq!(provider.counts(), (1, 2, 0));
}

fn assert_exchange(exchange: &capable::Exchange, operation: &str, poll: bool) {
    assert_eq!(
        exchange.headers["authorization"],
        format!("Bearer {SERVICE_TOKEN}")
    );
    assert_eq!(exchange.headers["x-hito-plugin-protocol"], "1");
    assert_eq!(exchange.body["operation_id"], operation);
    assert_eq!(exchange.body["recording_id"], capable::RECORDING);
    assert_eq!(exchange.body["source_id"], capable::SOURCE);
    let mut keys = exchange
        .body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    if poll {
        assert_eq!(
            keys,
            ["handle", "operation_id", "recording_id", "source_id"]
        );
        assert_eq!(exchange.body["handle"], capable::HANDLE);
    } else {
        assert_eq!(keys, ["operation_id", "recording_id", "source_id"]);
    }
}

fn assert_private_status(status: &Value) {
    let status = status.to_string();
    for private in [
        SERVICE_TOKEN,
        capable::RECORDING,
        capable::SOURCE,
        capable::HANDLE,
    ] {
        assert!(!status.contains(private));
    }
}

struct RuntimeHost {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl RuntimeHost {
    async fn start(services: hito_core::Services) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = hito_server::Config {
            origin: format!("http://{address}"),
            ..Default::default()
        };
        let (stop, stopped) = oneshot::channel();
        let app = hito_server::router_with_config(services.clone(), config);
        let task = tokio::spawn(async move {
            hito_server::serve(listener, app, services, async {
                let _ = stopped.await;
            })
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

    async fn finish(&mut self) {
        self.request_shutdown();
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(Duration::from_secs(7), &mut task).await {
                Ok(result) => {
                    result.unwrap();
                }
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    panic!("the real host must join its HTTP and preparation worker shutdown");
                }
            }
            assert!(tokio::net::TcpStream::connect(self.address).await.is_err());
        }
    }
}
