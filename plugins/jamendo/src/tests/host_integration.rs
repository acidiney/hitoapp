#[path = "support/host_actions.rs"]
mod actions;
#[path = "support/host.rs"]
mod host;

use super::fixture;
use actions::{adopt, assert_metadata_only, assert_private_transport_absent, audio, call};
use host::with_host;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::{future::pending, time::Duration};

#[tokio::test]
async fn jamendo_credits_survive_host_adoption_and_reopening_without_retrieving_audio() {
    with_host(async |owned| {
        let (token, recording, metadata) =
            adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
        assert_metadata_only(&owned.provider, owned.services()).await;
        owned.reopen().await;
        let retained = call(
            &owned.client,
            owned.server(),
            Method::GET,
            &format!("api/v1/recordings/{recording}/metadata"),
            Value::Null,
            Some(&token),
            StatusCode::OK,
        )
        .await;
        assert!(
            retained == metadata,
            "reopening must retain the exact credits"
        );
        assert_metadata_only(&owned.provider, owned.services()).await;
    })
    .await;
}

#[tokio::test]
async fn jamendo_host_playback_and_seek_return_owned_audio_without_a_persistent_copy() {
    with_host(async |owned| {
        let (token, recording, _) =
            adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
        let sources = call(
            &owned.client,
            owned.server(),
            Method::GET,
            &format!("api/v1/recordings/{recording}/sources"),
            Value::Null,
            Some(&token),
            StatusCode::OK,
        )
        .await;
        assert_eq!(sources.as_array().unwrap().len(), 1);
        assert!(sources[0]["ready"] == true, "owned source must be ready");
        assert_private_transport_absent(&sources);
        assert_metadata_only(&owned.provider, owned.services()).await;
        let source = sources[0]["id"].as_str().unwrap();
        let response = audio(owned, &token, source, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["content-type"] == "audio/wav",
            "unexpected host audio response header"
        );
        assert!(
            response.headers()["cache-control"] == "private, no-store",
            "unexpected host audio response header"
        );
        assert!(response.headers().get("set-cookie").is_none());
        assert!(response.headers().get("location").is_none());
        let bytes = response
            .bytes()
            .await
            .unwrap_or_else(|_| panic!("host audio body failed"));
        assert!(
            bytes.as_ref() == owned.provider.wav,
            "host must relay exact owned audio"
        );
        assert_eq!(owned.provider.media_requests(), 1);
        let seek = audio(owned, &token, source, Some("bytes=46-77")).await;
        assert_eq!(seek.status(), StatusCode::PARTIAL_CONTENT);
        assert!(
            seek.headers()["content-length"] == "32",
            "unexpected seek representation length"
        );
        assert!(
            seek.headers()["content-range"] == format!("bytes 46-77/{}", owned.provider.wav.len()),
            "unexpected seek representation range"
        );
        assert!(seek.headers().get("set-cookie").is_none());
        assert!(seek.headers().get("location").is_none());
        let bytes = seek
            .bytes()
            .await
            .unwrap_or_else(|_| panic!("host seek body failed"));
        assert!(
            bytes.as_ref() == &owned.provider.wav[46..=77],
            "seek must return the exact owned range"
        );
        assert_eq!(owned.provider.media_requests(), 2);
        assert_eq!(owned.provider.range_requests(), 1);
        assert_eq!(owned.provider.download_requests(), 0);
        call(
            &owned.client,
            owned.server(),
            Method::GET,
            &format!("api/v1/recordings/{recording}/download"),
            Value::Null,
            Some(&token),
            StatusCode::NOT_FOUND,
        )
        .await;
        assert!(
            !owned
                .services()
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap_or_else(|_| panic!("owned download worker failed"))
        );
    })
    .await;
}

#[tokio::test]
async fn jamendo_playback_only_policy_rejects_a_host_download_before_retrieving_audio() {
    with_host(async |owned| {
        let (token, recording, metadata) =
            adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
        let rejection = call(
            &owned.client,
            owned.server(),
            Method::POST,
            &format!("api/v1/recordings/{recording}/download"),
            json!({"request_id": uuid::Uuid::new_v4()}),
            Some(&token),
            StatusCode::CONFLICT,
        )
        .await;
        assert_private_transport_absent(&rejection);
        let message = rejection.to_string();
        assert!(message.contains("playback only"));
        for private in [
            fixture::QUERY,
            fixture::TITLE,
            fixture::ARTIST,
            fixture::ALBUM,
        ] {
            assert!(
                !message.contains(private),
                "policy errors must omit provider metadata"
            );
        }
        call(
            &owned.client,
            owned.server(),
            Method::GET,
            &format!("api/v1/recordings/{recording}/download"),
            Value::Null,
            Some(&token),
            StatusCode::NOT_FOUND,
        )
        .await;
        let retained = call(
            &owned.client,
            owned.server(),
            Method::GET,
            &format!("api/v1/recordings/{recording}/metadata"),
            Value::Null,
            Some(&token),
            StatusCode::OK,
        )
        .await;
        assert!(
            retained == metadata,
            "download rejection must preserve retained credits"
        );
        assert_metadata_only(&owned.provider, owned.services()).await;
    })
    .await;
}

#[tokio::test]
async fn jamendo_playlist_download_settles_as_policy_restricted_without_retrieving_audio() {
    with_host(async |owned| {
        let (token, recording, _) =
            adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
        let playlist = uuid::Uuid::new_v4();
        let entry = uuid::Uuid::new_v4();
        call(
            &owned.client, owned.server(), Method::POST, "api/v1/sync",
            json!({"device_id": "owned-jamendo-sync-client", "operations": [
                {"id": uuid::Uuid::new_v4(), "action": {"kind": "playlist_create", "playlist_id": playlist, "name": "Owned playlist"}},
                {"id": uuid::Uuid::new_v4(), "action": {"kind": "entry_insert", "playlist_id": playlist, "entry_id": entry, "recording_id": recording, "position": 0}}
            ]}), Some(&token), StatusCode::OK,
        ).await;
        let playlist_path = format!("api/v1/playlists/{playlist}");
        let before = call(
            &owned.client, owned.server(), Method::GET, &playlist_path,
            Value::Null, Some(&token), StatusCode::OK,
        ).await;
        let status_path = format!("{playlist_path}/download");
        let receipt = call(
            &owned.client, owned.server(), Method::POST, &status_path,
            json!({"request_id": uuid::Uuid::new_v4()}), Some(&token), StatusCode::ACCEPTED,
        ).await;
        let terminal = tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                let progressed = owned.services().downloads
                    .run_next_playlist_reconciliation_until(pending()).await.unwrap_or_else(|_| panic!("owned download reconciliation failed"));
                assert!(!owned.services().downloads.run_next_job_until(pending()).await.unwrap_or_else(|_| panic!("owned download reconciliation failed")));
                let status = call(
                    &owned.client, owned.server(), Method::GET, &status_path,
                    Value::Null, Some(&token), StatusCode::OK,
                ).await;
                if !progressed && status["desired_revision"] == status["reconciled_revision"] {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("owned playlist must settle within its deadline");
        assert!(terminal["retention_id"] == receipt["retention_id"]);
        assert_eq!(terminal["entries"]["items"].as_array().unwrap().len(), 1);
        let restricted = &terminal["entries"]["items"][0];
        assert!(restricted["entry_id"] == entry.to_string(), "unexpected restricted playlist outcome");
        assert!(restricted["recording_id"] == recording, "playlist recording identity changed");
        assert!(restricted["status"] == "unavailable", "unexpected restricted playlist outcome");
        assert!(restricted["error"] == "policy_restricted", "unexpected restricted playlist outcome");
        assert!(restricted["source_id"].is_null());
        let after = call(
            &owned.client, owned.server(), Method::GET, &playlist_path,
            Value::Null, Some(&token), StatusCode::OK,
        ).await;
        assert!(after == before, "policy restriction must preserve the playlist");
        call(
            &owned.client, owned.server(), Method::GET,
            &format!("api/v1/recordings/{recording}/download"),
            Value::Null, Some(&token), StatusCode::NOT_FOUND,
        ).await;
        assert_metadata_only(&owned.provider, owned.services()).await;
    }).await;
}

#[tokio::test]
async fn cancelling_partial_startup_joins_resources_before_removing_storage() {
    let observed = host::Observation::default();
    let gate = std::sync::Arc::new(host::StartupGate::new(false));
    let mut workflow = Box::pin(host::observed_host(
        async |_| {
            panic!("startup cancellation must prevent the workflow");
        },
        Some(gate.clone()),
        observed.clone(),
    ));
    tokio::select! {
        permit = gate.entered.acquire() => permit.unwrap_or_else(|_| panic!("owned startup control failed")).forget(),
        _ = &mut workflow => panic!("owned startup finished before its cancellation boundary"),
        _ = tokio::time::sleep(Duration::from_secs(5)) => panic!("owned startup did not reach its cancellation boundary"),
    }
    assert!(
        observed
            .directory
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .exists(),
        "storage must remain during startup"
    );
    drop(workflow);
    assert_settled(&observed, host::Outcome::Cancelled);
}

#[tokio::test]
async fn cancelling_an_active_request_joins_core_holders_before_removing_storage() {
    let observed = host::Observation::default();
    let hold = std::sync::Arc::new(host::RequestHold::new());
    let request_hold = hold.clone();
    let mut workflow = Box::pin(host::observed_host(
        async move |owned| {
            let (token, _, _) =
                adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
            owned.hold_requests(request_hold).await;
            let _ = owned
                .client
                .get(owned.server().base.join("api/v1/me").unwrap())
                .bearer_auth(token)
                .send()
                .await;
            panic!("owned held request must finish through cancellation");
        },
        None,
        observed.clone(),
    ));
    tokio::select! {
        permit = hold.entered.acquire() => permit.unwrap_or_else(|_| panic!("owned request control failed")).forget(),
        _ = &mut workflow => panic!("owned request finished before its cancellation boundary"),
        _ = tokio::time::sleep(Duration::from_secs(5)) => panic!("owned request did not reach its cancellation boundary"),
    }
    assert!(
        hold.active.load(std::sync::atomic::Ordering::SeqCst) == 1,
        "owned request must hold Core"
    );
    drop(workflow);
    assert_settled(&observed, host::Outcome::Cancelled);
    assert!(
        hold.active.load(std::sync::atomic::Ordering::SeqCst) == 0,
        "Core holders must retire before cancellation returns"
    );
    assert!(
        !hold
            .early_storage_release
            .load(std::sync::atomic::Ordering::SeqCst),
        "storage was released while a request held Core"
    );
}

#[tokio::test]
async fn partial_startup_and_workflow_panics_remain_failures_after_joined_cleanup() {
    let startup = host::Observation::default();
    let report = host::observed_host(
        async |_| {
            panic!("partial startup must not invoke the workflow");
        },
        Some(std::sync::Arc::new(host::StartupGate::new(true))),
        startup.clone(),
    )
    .await;
    assert!(
        report.outcome == host::Outcome::Panicked,
        "startup failure must survive cleanup"
    );
    assert_settled(&startup, host::Outcome::Panicked);
    assert!(
        report.connections.clean(),
        "partial startup panic cleanup must be clean"
    );
    let workflow = host::Observation::default();
    let report = host::observed_host(
        async |owned| {
            let _ = adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
            panic!("owned workflow control failure");
        },
        None,
        workflow.clone(),
    )
    .await;
    assert!(
        report.outcome == host::Outcome::Panicked,
        "workflow failure must survive cleanup"
    );
    assert_settled(&workflow, host::Outcome::Panicked);
    assert!(
        report.connections.clean(),
        "workflow panic cleanup must be clean"
    );
}

fn assert_settled(observed: &host::Observation, outcome: host::Outcome) {
    let report = observed
        .report
        .lock()
        .unwrap()
        .as_ref()
        .copied()
        .expect("owned cleanup report missing");
    assert!(
        report.outcome == outcome,
        "owned workflow lost its failure outcome"
    );
    assert!(
        report.connections.accepted == report.connections.joined
            && report.jobs_settled
            && report.database_retired
            && !report.database_deadline_exceeded,
        "owned connections and Core jobs must settle"
    );
    assert!(
        !observed
            .directory
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .exists(),
        "owned storage must be released after settlement"
    );
    for address in observed.servers.lock().unwrap().iter() {
        let address = std::net::SocketAddr::new(
            std::net::Ipv4Addr::LOCALHOST.into(),
            address.port().unwrap(),
        );
        assert!(
            std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
            "owned listener survived settlement"
        );
    }
}

#[test]
fn private_values_never_reach_panic_output_from_new_http_helpers() {
    for kind in ["attribution", "send", "body", "foreign_url"] {
        let (status, output) = crate::contract::server::disposable_child(
            "contract::host_integration::private_diagnostic_child",
            kind,
        );
        assert!(!status.success(), "negative diagnostic control must fail");
        let expected = match kind {
            "attribution" => "retained Jamendo attribution did not match",
            "send" => "owned host HTTP request failed",
            "foreign_url" => "host response exposed private transport data",
            _ => "owned host HTTP body failed",
        };
        assert!(
            output.contains(expected),
            "negative diagnostic control missed its boundary"
        );
        assert!(
            !output.contains("private-owned-diagnostic-sentinel"),
            "private input reached a panic hook"
        );
    }
}

#[tokio::test]
async fn private_diagnostic_child() {
    let kind = std::env::var("HITO_JAMENDO_OWNED_CHILD").ok();
    let Some(kind) = kind.filter(|kind| {
        matches!(
            kind.as_str(),
            "attribution" | "send" | "body" | "foreign_url"
        )
    }) else {
        return;
    };
    if kind == "foreign_url" {
        actions::assert_private_transport_absent(
            &json!({"unexpected": "https://private-owned-diagnostic-sentinel.invalid/track"}),
        );
        return;
    }
    if kind == "attribution" {
        actions::assert_attribution(
            &json!({"attribution": {"creators": ["private-owned-diagnostic-sentinel"]}}),
        );
        return;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let router = axum::Router::new().route(
        "/private-owned-diagnostic-sentinel",
        axum::routing::get(|| async {
            axum::response::Response::new(axum::body::Body::from_stream(
                futures_util::stream::pending::<Result<axum::body::Bytes, std::io::Error>>(),
            ))
        }),
    );
    let server = fixture::Server::start(router).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| panic!("owned diagnostic client failed"));
    if kind == "send" {
        let base = server.base.clone();
        assert!(
            server.shutdown().await,
            "owned diagnostic listener must stop"
        );
        let response = client
            .get(base.join("private-owned-diagnostic-sentinel").unwrap())
            .send()
            .await;
        actions::received_response(response, StatusCode::OK).await;
    } else {
        let response = client
            .get(
                server
                    .base
                    .join("private-owned-diagnostic-sentinel")
                    .unwrap(),
            )
            .send()
            .await
            .unwrap_or_else(|_| panic!("owned diagnostic headers failed"));
        assert!(
            !server.shutdown().await,
            "owned held body must preserve its deadline outcome"
        );
        actions::json_body(response).await;
    }
}

#[test]
fn database_resources_retire_before_host_cleanup_acknowledgement() {
    database_child("sqlite_close");
}

#[test]
fn cancelled_database_open_retires_resources_before_host_cleanup_acknowledgement() {
    database_child("sqlite_open_cancel");
}

#[test]
fn database_retirement_deadline_stays_failed_after_resources_release() {
    database_child("sqlite_deadline");
}

fn database_child(kind: &str) {
    let (status, output) = crate::contract::server::disposable_child(
        "contract::host_integration::database_retirement_child",
        kind,
    );
    assert!(
        output.contains("owned_database_control stage=cleanup_returned"),
        "database control missed its cleanup boundary"
    );
    assert!(
        status.success(),
        "host cleanup acknowledged live SQLite resource leases"
    );
}

struct DatabaseRelease {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<bool>>,
    gate: std::sync::Arc<hito_core::test_support::database_owner::RetirementGate>,
}

impl DatabaseRelease {
    fn start(
        observed: host::Observation,
        gate: std::sync::Arc<hito_core::test_support::database_owner::RetirementGate>,
    ) -> Self {
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let held = gate.clone();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|_| panic!("owned database control runtime failed"));
            runtime.block_on(async {
                tokio::select! {
                    _ = &mut stopped => { held.release(); false }
                    result = async {
                        observed.database_ready.acquire().await.unwrap_or_else(|_| panic!("owned database observation closed")).forget();
                        let database = observed.database.lock().unwrap().as_ref().unwrap().clone();
                        database.retirement_started().await;
                        held.entered().await;
                        if observed.database_deadline.is_some() { database.retirement_deadline().await; }
                        let retained = observed.directory.lock().unwrap().as_ref().unwrap().exists();
                        held.release();
                        retained
                    } => result,
                }
            })
        });
        Self {
            stop: Some(stop),
            thread: Some(thread),
            gate,
        }
    }

    fn finish(mut self) -> bool {
        self.thread.take().unwrap().join().unwrap_or(false)
    }
}

impl Drop for DatabaseRelease {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.gate.release();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[tokio::test]
async fn database_retirement_child() {
    use hito_core::test_support::database_owner::{OpeningGate, RetirementGate};
    let kind = std::env::var("HITO_JAMENDO_OWNED_CHILD").ok();
    let Some(kind) = kind.filter(|kind| {
        matches!(
            kind.as_str(),
            "sqlite_close" | "sqlite_open_cancel" | "sqlite_deadline"
        )
    }) else {
        return;
    };
    let retirement = std::sync::Arc::new(RetirementGate::new());
    let opening = (kind == "sqlite_open_cancel").then(|| std::sync::Arc::new(OpeningGate::new()));
    let observed = host::Observation {
        database_retirement: Some(retirement.clone()),
        database_opening: opening.clone(),
        database_deadline: (kind == "sqlite_deadline").then_some(Duration::from_millis(20)),
        ..Default::default()
    };
    let release = DatabaseRelease::start(observed.clone(), retirement);
    if let Some(opening) = opening {
        let mut workflow = Box::pin(host::observed_host(
            async |_| {
                panic!("cancelled Core opening must not invoke the workflow");
            },
            None,
            observed.clone(),
        ));
        tokio::select! {
            _ = opening.entered() => {}
            _ = &mut workflow => panic!("Core opening passed its publication gate"),
            _ = tokio::time::sleep(Duration::from_secs(4)) => panic!("Core opening did not reach its publication gate"),
        }
        assert!(
            !observed
                .database
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .pool_published(),
            "cancelled Core opening must not publish a pool"
        );
        drop(workflow);
    } else {
        let report = host::observed_host(
            async |owned| {
                let _ = adopt(&owned.client, owned.server(), owned.plugin.base.as_str()).await;
            },
            None,
            observed.clone(),
        )
        .await;
        assert!(
            report.outcome == host::Outcome::Complete,
            "owned database workflow failed"
        );
    }
    let report = observed
        .report
        .lock()
        .unwrap()
        .as_ref()
        .copied()
        .expect("owned database cleanup report missing");
    assert!(
        report.database_retired,
        "database retirement acknowledgement missing"
    );
    assert!(
        report.database_deadline_exceeded == (kind == "sqlite_deadline"),
        "database retirement lost its deadline failure"
    );
    println!("owned_database_control stage=cleanup_returned");
    assert!(
        observed
            .database
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .active_resources()
            == 0,
        "owned cleanup returned before SQLite worker resource retirement"
    );
    assert!(
        release.finish(),
        "owned storage was released before SQLite worker resource retirement"
    );
}
