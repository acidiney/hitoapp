#![cfg(feature = "test-support")]

#[path = "support/restart_catalog_entries.rs"]
pub mod entries;
#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
pub mod fixtures;
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
pub mod http;
#[path = "../../hito-debrid-torbox/tests/audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "support/restart_catalog_protocol.rs"]
pub mod protocol;
#[path = "../../hito-debrid-torbox/tests/support/mod.rs"]
pub mod provider;
#[path = "support/restart_workflow.rs"]
pub mod ready_workflow;
#[path = "../../hito-debrid-torbox/tests/audio/scripted_media.rs"]
pub mod scripted_media;
#[path = "support/state_inputs.rs"]
mod state_inputs;
#[path = "support/restart_catalog_workflow.rs"]
pub mod workflow;

use hito_plugin_torbox::{CuratedWavRecording, StateError};
use provider::reply::Reply;
use reqwest::StatusCode;
use std::time::Duration;
use tokio::time::Instant;

pub const COOKIE: &str = "owned-private-torbox-escaped-binding-cookie";

#[tokio::test]
async fn client_valid_maximum_escaped_filename_preserves_ready_receipts_across_reopen() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let name = "\"\\".repeat(2048);
        assert_eq!(name.len(), 4096);
        let observed = || Reply::json(fixtures::observed(true, true, &name));
        let fixture = workflow::Fixture::scripted(vec![
            Reply::json(protocol::control_observation()),
            Reply::json(fixtures::acknowledgment()),
            Reply::json(protocol::control_observation()),
            observed(),
            Reply::json(fixtures::acknowledgment()),
            observed(),
            observed(),
            Reply::json(protocol::control_observation()),
        ], 4).await;
        let first = fixture.open_with(recordings(&name), Duration::from_secs(4), None).await
            .unwrap_or_else(|_| panic!("the client-valid escaped binding must initialize"));
        let control = first.prepare_target(entries::ADDED_RECORDING, entries::ADDED_SOURCE).await;
        let mut windows = Vec::new();
        let started = Instant::now();
        ready_workflow::complete(first.audio_source(entries::ADDED_SOURCE).await).await;
        windows.push((started, Instant::now()));
        let request = workflow::request(entries::RECORDING, entries::SOURCE);
        let submitted = first.submit(&request).await;
        let control_status = first.status(&control).await;
        let control_replay = first.replay(&control).await;
        if submitted.0 != StatusCode::OK {
            let prepare_status = submitted.0.as_u16();
            let retained_status = control_status.0.as_u16();
            let unavailable = serde_json::from_slice::<serde_json::Value>(&submitted.1)
                .is_ok_and(|body| body == serde_json::json!({"error":"The source is unavailable"}));
            let close_outcome = match first.close_result().await {
                Ok(()) => "joined",
                Err(StateError::InvalidState) => "invalid_state",
                Err(_) => "other_error",
            };
            let (requests, media_requests) = fixture.close().await;
            eprintln!("escaped_binding_evidence prepare_status={prepare_status} retained_status={retained_status} closed_unavailable={unavailable} owner_close_outcome={close_outcome} provider_requests={} mint_requests={} media_requests={media_requests}",
                requests.len(), mint_count(&requests));
            assert_eq!(requests.len(), 3);
            assert_eq!(mint_count(&requests), 1);
            assert_eq!(media_requests, 1);
            assert_requests(&requests, &windows);
            assert_eq!(submitted.0, StatusCode::OK, "every existing client-valid filename must prepare without poisoning the retained Ready control");
            return;
        }
        let saved = workflow::saved(&request, &submitted)
            .expect("the escaped binding must return its actual stable public handle");
        workflow::assert_ready_replay(submitted, &saved);
        workflow::assert_ready_status(first.status(&saved).await, &saved);
        workflow::assert_ready_status(control_status, &control);
        workflow::assert_ready_replay(control_replay, &control);
        let started = Instant::now();
        ready_workflow::complete(first.audio().await).await;
        windows.push((started, Instant::now()));
        first.close().await;

        let replacement = fixture.open_with(recordings(&name), Duration::from_secs(4), None).await
            .unwrap_or_else(|_| panic!("the unchanged escaped binding must reopen after joined shutdown"));
        workflow::assert_ready_status(replacement.status(&saved).await, &saved);
        workflow::assert_ready_replay(replacement.replay(&saved).await, &saved);
        workflow::assert_ready_status(replacement.status(&control).await, &control);
        workflow::assert_ready_replay(replacement.replay(&control).await, &control);
        for source in [entries::SOURCE, entries::ADDED_SOURCE] {
            let started = Instant::now();
            ready_workflow::complete(replacement.audio_source(source).await).await;
            windows.push((started, Instant::now()));
        }
        replacement.close().await;
        let (requests, media_requests) = fixture.close().await;
        eprintln!("escaped_binding_evidence prepare_status=200 retained_status=200 owner_close_outcome=joined provider_requests={} mint_requests={} media_requests={media_requests}",
            requests.len(), mint_count(&requests));
        assert_eq!(requests.len(), 8);
        assert_eq!(mint_count(&requests), 2);
        assert_eq!(media_requests, 4);
        assert_eq!(windows.len(), 4);
        assert_requests(&requests, &windows);
    }).await.expect("the public escaped-binding workflow exceeded its deadline");
}

fn recordings(name: &str) -> Vec<CuratedWavRecording> {
    let mut entries = entries::original(true);
    let selected = entries
        .iter_mut()
        .find(|entry| entry.source_id == entries::SOURCE)
        .expect("the original source identity must remain present");
    selected.binding.file_name = name.into();
    assert!(selected.binding.validate().is_ok());
    entries
}

fn mint_count(requests: &[provider::Request]) -> usize {
    requests
        .iter()
        .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
        .count()
}

fn assert_requests(requests: &[provider::Request], windows: &[(Instant, Instant)]) {
    let control = "/v1/api/torrents/mylist?id=8124&bypass_cache=true";
    let selected = "/v1/api/torrents/mylist?id=7123&bypass_cache=true";
    let targets = [
        control,
        "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=8124&file_id=19&zip_link=false&redirect=false&append_name=false",
        control,
        selected,
        "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false",
        selected,
        selected,
        control,
    ];
    assert!(requests.len() <= targets.len());
    let authorization = format!("Bearer {}", fixtures::TOKEN);
    for (index, request) in requests.iter().enumerate() {
        assert!(
            request.method == "GET"
                && request.server_name == "api.torbox.app"
                && request.body.is_empty()
        );
        assert!(request.target == targets[index]);
        for header in [
            "cookie",
            "referer",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(header).is_none());
        }
        assert!(
            request.header("authorization")
                == if matches!(index, 1 | 4) {
                    None
                } else {
                    Some(authorization.as_str())
                }
        );
        let window = match index {
            2 => Some(0),
            5 => Some(1),
            6 => Some(2),
            7 => Some(3),
            _ => None,
        };
        if let Some(window) = window {
            let (started, finished) = windows[window];
            assert!((started..=finished).contains(&request.received_at));
        }
    }
}
