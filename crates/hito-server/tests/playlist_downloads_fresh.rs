#[path = "fixtures/playlist_download_fixture.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "fixtures/playlist_download_fresh_fixture.rs"]
mod fresh;
#[path = "fixtures/download_host.rs"]
mod host;
#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::http::StatusCode;
use fresh::{FreshFollow, Item, occurrence};
use futures_util::FutureExt;
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_playlist_download_transfers_ready_sources_and_follows_membership_without_another_click()
{
    let fixture = fixture::Fixture::start().await;
    let gate = fixture.provider.hold("v1/audio").await;
    let mut host = host::Host::start(fixture.services.clone()).await;
    let outcome = AssertUnwindSafe(exercise(&fixture, host.address, &gate))
        .catch_unwind()
        .await;
    gate.open();
    host.request_shutdown();
    assert!(
        host.join().await,
        "the owned host must join HTTP and all background work"
    );
    assert!(
        std::fs::read(fixture.directory.path().join("music/original.wav")).unwrap()
            == fixture.original,
        "playlist transfers and removals must preserve user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(fixture: &fixture::Fixture, address: SocketAddr, gate: &provider::Gate) {
    let follow = FreshFollow::prepare(fixture, address).await;
    let selection = &follow.selection;
    let token = &selection.token;
    let before = fixture.saved_state(address, selection).await;
    assert_eq!(before["catalog"]["total"], 3);
    assert_eq!(before["playlist"]["entries"].as_array().unwrap().len(), 1);
    assert!(before["metadata"]["title"]["personal"] == "My retained title");
    assert!(before["metadata"]["artist"]["personal"] == "My retained artist");
    assert!(before["activity"]["history"][0]["position_ms"] == 700);
    assert!(before["activity"]["history"][0]["speed"] == 1.25);
    assert_no_individual_downloads(address, &follow).await;
    assert!(fixture.provider.requests("v1/audio").await.is_empty());
    let download_path = format!("/api/v1/playlists/{}/download", selection.playlist);
    assert_eq!(
        call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
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
    let receipt = response.json();
    assert!(receipt["request_id"] == request_id.to_string());
    assert!(receipt["playlist_id"] == selection.playlist);
    let retention = receipt["retention_id"].as_str().unwrap();
    Uuid::parse_str(retention).unwrap();
    tokio::time::timeout(Duration::from_secs(8), gate.entered())
        .await
        .expect("an explicit playlist Download must request owned audio A without an individual Download");
    let requests = fixture.provider.requests("v1/audio").await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1 == json!({"source_id": fresh::SOURCE_A, "_method": "GET"}));
    gate.release();

    let a_ready = saved(
        address,
        &follow,
        retention,
        &[
            (&follow.a_entries[0], &selection.recording, "ready"),
            (&follow.a_entries[1], &selection.recording, "ready"),
            (
                &follow.unmatched_entry,
                &follow.unmatched_recording,
                "waiting_for_source",
            ),
        ],
    )
    .await;
    let a_source = entry(&a_ready, &follow.a_entries[0])["source_id"]
        .as_str()
        .unwrap();
    assert!(entry(&a_ready, &follow.a_entries[1])["source_id"] == a_source);
    assert_ne!(a_source, selection.provider_source);
    assert_audio(address, token, a_source, &fixture.audio).await;
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 1);
    assert_no_individual_downloads(address, &follow).await;
    assert!(fixture.saved_state(address, selection).await == before);

    follow.serve_b_audio(fixture).await;
    let detail = fresh::refresh(
        fixture,
        address,
        selection,
        "add-ready-B",
        &[Item::First, Item::Duplicate, Item::Unmatched, Item::Added],
    )
    .await;
    assert_eq!(detail["entries"].as_array().unwrap().len(), 4);
    assert!(occurrence(&detail, "first")["entry_id"] == follow.a_entries[0]);
    assert!(occurrence(&detail, "duplicate")["entry_id"] == follow.a_entries[1]);
    assert!(occurrence(&detail, "unmatched")["entry_id"] == follow.unmatched_entry);
    let added = occurrence(&detail, "added");
    assert!(added["recording_id"] == follow.b_recording);
    let b_entry = added["entry_id"].as_str().unwrap();
    Uuid::parse_str(b_entry).unwrap();
    assert!(!follow.a_entries.iter().any(|id| id == b_entry));
    assert_ne!(b_entry, follow.unmatched_entry);
    tokio::time::timeout(Duration::from_secs(8), gate.entered())
        .await
        .expect("an accepted Follow addition must request already adopted audio B without another Download");
    let requests = fixture.provider.requests("v1/audio").await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1].1 == json!({"source_id": fresh::SOURCE_B, "_method": "GET"}));
    gate.release();
    let both_ready = saved(
        address,
        &follow,
        retention,
        &[
            (&follow.a_entries[0], &selection.recording, "ready"),
            (&follow.a_entries[1], &selection.recording, "ready"),
            (
                &follow.unmatched_entry,
                &follow.unmatched_recording,
                "waiting_for_source",
            ),
            (b_entry, &follow.b_recording, "ready"),
        ],
    )
    .await;
    assert!(
        both_ready["desired_revision"].as_u64().unwrap()
            > a_ready["desired_revision"].as_u64().unwrap()
    );
    assert!(entry(&both_ready, &follow.a_entries[0])["source_id"] == a_source);
    assert!(entry(&both_ready, &follow.a_entries[1])["source_id"] == a_source);
    let b_source = entry(&both_ready, b_entry)["source_id"].as_str().unwrap();
    assert_ne!(b_source, a_source);
    assert_ne!(b_source, follow.b_provider_source);
    assert_audio(address, token, a_source, &fixture.audio).await;
    assert_audio(address, token, b_source, &follow.b_audio).await;
    assert_no_individual_downloads(address, &follow).await;
    assert!(fixture.saved_state(address, selection).await == before);

    let detail = fresh::refresh(
        fixture,
        address,
        selection,
        "remove-only-A",
        &[Item::Unmatched, Item::Added],
    )
    .await;
    assert_eq!(detail["entries"].as_array().unwrap().len(), 2);
    assert!(occurrence(&detail, "added")["entry_id"] == b_entry);
    let b_retained = saved(
        address,
        &follow,
        retention,
        &[
            (
                &follow.unmatched_entry,
                &follow.unmatched_recording,
                "waiting_for_source",
            ),
            (b_entry, &follow.b_recording, "ready"),
        ],
    )
    .await;
    assert!(
        b_retained["desired_revision"].as_u64().unwrap()
            > both_ready["desired_revision"].as_u64().unwrap()
    );
    assert!(entry(&b_retained, b_entry)["source_id"] == b_source);
    assert_retired(
        address,
        token,
        &selection.recording,
        &selection.provider_source,
        a_source,
    )
    .await;
    assert_audio(address, token, b_source, &follow.b_audio).await;
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 2);
    assert!(fixture.saved_state(address, selection).await == before);

    let detail = fresh::refresh(
        fixture,
        address,
        selection,
        "remove-B-keep-unmatched",
        &[Item::Unmatched],
    )
    .await;
    assert_eq!(detail["entries"].as_array().unwrap().len(), 1);
    assert!(occurrence(&detail, "unmatched")["entry_id"] == follow.unmatched_entry);
    let released = saved(
        address,
        &follow,
        retention,
        &[(
            &follow.unmatched_entry,
            &follow.unmatched_recording,
            "waiting_for_source",
        )],
    )
    .await;
    assert!(
        released["desired_revision"].as_u64().unwrap()
            > b_retained["desired_revision"].as_u64().unwrap()
    );
    assert_retired(
        address,
        token,
        &follow.b_recording,
        &follow.b_provider_source,
        b_source,
    )
    .await;
    assert_no_individual_downloads(address, &follow).await;
    assert!(fixture.saved_state(address, selection).await == before);
    assert_eq!(fixture.provider.requests("v1/audio").await.len(), 2);
}

async fn saved(
    address: SocketAddr,
    follow: &FreshFollow,
    retention: &str,
    expected: &[(&str, &str, &str)],
) -> Value {
    let path = format!("/api/v1/playlists/{}/download", follow.selection.playlist);
    let state = fixture::wait_for_json(address, &follow.selection.token, &path, |state| {
        state["desired_revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
            && state["desired_revision"] == state["reconciled_revision"]
            && state["entries"]["items"].as_array().is_some_and(|items| {
                items.len() == expected.len()
                    && expected.iter().all(|(id, recording, status)| {
                        items.iter().any(|item| {
                            item["entry_id"] == *id
                                && item["recording_id"] == *recording
                                && item["status"] == *status
                        })
                    })
            })
    })
    .await;
    assert!(state["retention_id"] == retention);
    assert!(state["playlist_id"] == follow.selection.playlist);
    assert!(state["host_device_id"] == fixture::HOST);
    assert!(state["entries"]["next_cursor"].is_null());
    for (id, _, status) in expected {
        let item = entry(&state, id);
        assert!(item["error"].is_null());
        if *status == "ready" {
            Uuid::parse_str(item["source_id"].as_str().unwrap()).unwrap();
        } else {
            assert!(item["source_id"].is_null());
        }
    }
    state
}

fn entry<'a>(state: &'a Value, id: &str) -> &'a Value {
    state["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["entry_id"] == id)
        .expect("download status must identify the authoritative playlist occurrence")
}

async fn assert_no_individual_downloads(address: SocketAddr, follow: &FreshFollow) {
    for recording in [
        &follow.selection.recording,
        &follow.b_recording,
        &follow.unmatched_recording,
    ] {
        assert_eq!(
            call(
                address,
                "GET",
                &format!("/api/v1/recordings/{recording}/download"),
                None,
                Some(&follow.selection.token)
            )
            .await
            .status,
            StatusCode::NOT_FOUND,
            "playlist Download must not create an individual retention"
        );
    }
}

async fn assert_audio(address: SocketAddr, token: &str, source: &str, expected: &[u8]) {
    let response = call(
        address,
        "GET",
        &format!("/api/v1/sources/{source}/audio"),
        None,
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert!(
        response.body == expected,
        "the managed source must serve its exact owned WAV"
    );
}

async fn assert_retired(
    address: SocketAddr,
    token: &str,
    recording: &str,
    provider_source: &str,
    source: &str,
) {
    let path = format!("/api/v1/sources/{source}/audio");
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let response = call(address, "GET", &path, None, Some(token)).await;
            if response.status == StatusCode::NOT_FOUND {
                break;
            }
            assert_eq!(response.status, StatusCode::OK);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("removing the last authoritative occurrence must release managed playback");
    let sources = get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/sources"),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["id"] == provider_source);
}
