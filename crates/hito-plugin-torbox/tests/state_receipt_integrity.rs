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

use hito_plugin_torbox::{InitError, StateError};
use provider::reply::Reply;
use sqlx::{ConnectOptions, Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{path::Path, time::Duration};
use uuid::Uuid;

pub const COOKIE: &str = "owned-private-torbox-receipt-integrity-cookie";

#[tokio::test]
async fn deleted_unknown_receipt_rejects_reopening_before_provider_contact() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut held, received, _hold_acknowledgement) = Reply::paused(fixtures::acknowledgment());
        let peer_closed = held.observe_peer_close();
        let fixture = workflow::Fixture::scripted(
            vec![
                Reply::json(protocol::control_observation()),
                Reply::json(fixtures::acknowledgment()),
                Reply::json(fixtures::observed(true, true, "album/owned.wav")),
                held,
                Reply::json(protocol::control_observation()),
            ],
            1,
        )
        .await;
        let first = fixture.open_catalog(entries::Evolution::Restored).await
            .unwrap_or_else(|_| panic!("the original private state must open"));
        let ready = first.prepare_target(entries::ADDED_RECORDING, entries::ADDED_SOURCE).await;
        let operation = workflow::request(entries::RECORDING, entries::SOURCE);
        let (in_flight, completed) = {
            let mut pending = Box::pin(first.submit(&operation));
            tokio::select! {
                _ = &mut pending => panic!("the original mint must remain held before acknowledgement"),
                received = tokio::time::timeout(Duration::from_secs(2), received) => {
                    received.expect("mint must reach the owned provider").expect("the mint witness must remain owned");
                }
            }
            let in_flight = first.submit(&operation).await;
            let completed = tokio::time::timeout(Duration::from_secs(12), pending).await
                .expect("the lost acknowledgement must return through public HTTP");
            (in_flight, completed)
        };
        tokio::time::timeout(Duration::from_secs(2), peer_closed).await
            .expect("the timed-out mint must close its provider peer")
            .expect("the peer-close witness must remain owned");
        let saved = workflow::saved(&operation, &in_flight)
            .expect("the in-flight operation must expose its actual public handle");
        protocol::assert_unknown_submission(in_flight, &saved);
        protocol::assert_unknown_submission(completed, &saved);
        protocol::assert_unknown_status(first.status(&saved).await, &saved);
        workflow::assert_ready_status(first.status(&ready).await, &ready);
        workflow::assert_ready_replay(first.replay(&ready).await, &ready);
        let started = tokio::time::Instant::now();
        ready_workflow::complete(first.audio_source(entries::ADDED_SOURCE).await).await;
        let finished = tokio::time::Instant::now();
        first.close().await;

        let intact = fixture.open_catalog(entries::Evolution::Restored).await
            .unwrap_or_else(|_| panic!("the intact journal must reopen before fault injection"));
        protocol::assert_unknown_status(intact.status(&saved).await, &saved);
        protocol::assert_unknown_submission(intact.replay(&saved).await, &saved);
        workflow::assert_ready_status(intact.status(&ready).await, &ready);
        workflow::assert_ready_replay(intact.replay(&ready).await, &ready);
        intact.close().await;

        delete_unknown_receipt(&fixture.state_directory(), operation.operation_id, ready.request.operation_id).await;
        let outcome = match fixture.open_catalog(entries::Evolution::Restored).await {
            Err(InitError::State(StateError::InvalidState)) => "invalid_state",
            Err(_) => "other_error",
            Ok(unexpected) => {
                unexpected.close().await;
                "opened"
            }
        };
        let (requests, media_requests) = fixture.close().await;
        eprintln!("receipt_integrity_evidence reopen_outcome={outcome} deleted_receipts=1 provider_requests={} mint_requests={} media_requests={media_requests}",
            requests.len(), requests.iter().filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?")).count());
        protocol::assert_uncertain_requests(&requests, &[(started, finished)]);
        assert_eq!(media_requests, 1);
        assert_eq!(outcome, "invalid_state", "selective Unknown deletion must reject public Service::open");
    }).await.expect("the public selective-receipt integrity workflow exceeded its deadline");
}

async fn delete_unknown_receipt(directory: &Path, unknown: Uuid, ready: Uuid) {
    let key_file = directory
        .parent()
        .expect("the fixture state must have an owned parent")
        .join("candidate.key");
    let key = std::fs::read(&key_file).expect("the owned fixture key must remain readable");
    let options = SqliteConnectOptions::new()
        .filename(directory.join("preparation.sqlite"))
        .create_if_missing(false)
        .disable_statement_logging();
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|_| {
            panic!("the joined fixture database must open for the owned deletion fault")
        });
    let header: Vec<u8> =
        sqlx::query_scalar("SELECT mac FROM bootstrap_metadata WHERE singleton = 1")
            .fetch_one(&mut connection)
            .await
            .unwrap_or_else(|_| panic!("the original authenticated header must exist"));
    let ready_mac: Vec<u8> =
        sqlx::query_scalar("SELECT mac FROM operations WHERE operation_id = ?")
            .bind(ready.as_bytes().as_slice())
            .fetch_one(&mut connection)
            .await
            .unwrap_or_else(|_| panic!("the unrelated Ready receipt must remain stored"));
    let deleted = sqlx::query("DELETE FROM operations WHERE operation_id = ?")
        .bind(unknown.as_bytes().as_slice())
        .execute(&mut connection)
        .await
        .unwrap_or_else(|_| panic!("the owned selective deletion fault must complete"));
    assert_eq!(deleted.rows_affected(), 1);
    let remaining_header: Vec<u8> =
        sqlx::query_scalar("SELECT mac FROM bootstrap_metadata WHERE singleton = 1")
            .fetch_one(&mut connection)
            .await
            .unwrap_or_else(|_| panic!("the original authenticated header must remain"));
    let remaining_ready_mac: Vec<u8> =
        sqlx::query_scalar("SELECT mac FROM operations WHERE operation_id = ?")
            .bind(ready.as_bytes().as_slice())
            .fetch_one(&mut connection)
            .await
            .unwrap_or_else(|_| panic!("the unrelated Ready receipt must remain after deletion"));
    assert!(header == remaining_header && ready_mac == remaining_ready_mac);
    connection
        .close()
        .await
        .unwrap_or_else(|_| panic!("the owned deletion connection must join before reopening"));
    assert!(std::fs::read(key_file).expect("the retained fixture key must remain readable") == key);
}
