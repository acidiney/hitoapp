use hito_debrid_all_debrid::test_support::provider as support;

use hito_debrid_all_debrid::{
    AllDebridClient, DelayedFileReceipt, DelayedPoll, Error, MagnetFileBinding, PreparedFile,
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::{Fixture, Reply, logs::Capture};
use tokio::sync::Notify;
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-private-delayed-poll-key";
const CANDIDATE: &str =
    "https://api.alldebrid.com/owned-delayed-media?signature=private-delayed-signature";

#[tokio::test]
async fn delayed_receipt_polls_after_five_seconds_without_reunlocking_or_fetching_media() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let binding = MagnetFileBinding {
            magnet_id: 7123,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            magnet_bytes: 1200,
            file_path: vec!["album".into(), "owned.wav".into()],
            file_bytes: 400,
        };
        let before = serde_json::to_vec(&binding).unwrap();
        let fixture = Fixture::start(vec![
            serde_json::to_vec(&json!({
                "status": "success",
                "data": { "magnets": [{ "id": 7123, "size": 1200, "statusCode": 4 }] }
            }))
            .unwrap(),
            files(),
            serde_json::to_vec(&json!({
                "status": "success",
                "data": { "delayed": 2468, "filename": "owned.wav", "filesize": 400 }
            }))
            .unwrap(),
            serde_json::to_vec(&json!({
                "status": "success", "data": { "status": 1, "time_left": 2 }
            }))
            .unwrap(),
            serde_json::to_vec(&json!({
                "status": "success", "data": { "status": 2, "time_left": 0, "link": CANDIDATE }
            }))
            .unwrap(),
        ])
        .await;
        let capture = Capture::default();
        let subscriber = capture.dispatch();
        let client =
            AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
                .expect("the owned TLS client must initialize");
        let PreparedFile::Delayed(receipt) = client
            .prepare_bound_file(&binding)
            .with_subscriber(subscriber.clone())
            .await
            .expect("the exact file must produce one delayed acknowledgment")
        else {
            panic!("the owned delayed acknowledgment must remain delayed")
        };
        let stored = serde_json::to_vec(&receipt).unwrap();
        drop(client);
        let mut receipt: DelayedFileReceipt = serde_json::from_slice(&stored)
            .expect("the trusted owned delayed snapshot must reopen");
        assert!(receipt.binding() == &binding);
        let client =
            AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
                .expect("the recreated owned TLS client must initialize");

        let processing = client
            .poll_delayed(&mut receipt)
            .with_subscriber(subscriber.clone())
            .await;
        let candidate = client
            .poll_delayed(&mut receipt)
            .with_subscriber(subscriber)
            .await;
        let requests = fixture.close().await;

        match processing.expect("the acknowledged delayed ID must report processing") {
            DelayedPoll::Processing {
                poll_after_seconds,
                time_left_seconds,
            } => {
                assert_eq!(poll_after_seconds, 5);
                assert_eq!(time_left_seconds, 2);
            }
            DelayedPoll::Candidate(_) => {
                panic!("processing must not fabricate a generated candidate")
            }
        }
        let candidate =
            match candidate.expect("a completed delayed ID must return an opaque candidate") {
                DelayedPoll::Candidate(candidate) => candidate,
                DelayedPoll::Processing { .. } => {
                    panic!("completed delayed status must retain its opaque candidate")
                }
            };
        assert!(candidate.binding() == &binding);
        assert!(receipt.binding() == &binding);
        assert!(serde_json::to_vec(&binding).unwrap() == before);
        assert_eq!(requests.len(), 5);
        assert!(
            requests[3]
                .received_at
                .duration_since(requests[2].received_at)
                >= Duration::from_secs(5)
        );
        assert!(
            requests[4]
                .received_at
                .duration_since(requests[3].received_at)
                >= Duration::from_secs(5)
        );
        for (index, request) in requests.iter().enumerate() {
            assert!(request.method == "POST");
            assert!(request.server_name == "api.alldebrid.com");
            assert!(request.header("host") == Some("api.alldebrid.com"));
            assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            assert!(request.header("content-type") == Some("application/x-www-form-urlencoded"));
            assert!(request.header("accept-encoding") == Some("identity"));
            assert!(request.header("cookie").is_none());
            let (target, body) = match index {
                0 => ("/v4.1/magnet/status", "id=7123"),
                1 => ("/v4/magnet/files", "id%5B%5D=7123"),
                2 => (
                    "/v4/link/unlock",
                    "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-selected",
                ),
                3 | 4 => ("/v4/link/delayed", "id=2468"),
                _ => unreachable!(),
            };
            assert!(request.target == target);
            assert!(request.body == body.as_bytes());
        }
        capture.assert_workflows(
            &[
                ("bound_file_preparation", "delayed", 1),
                ("delayed_file_poll", "processing", 1),
                ("delayed_file_poll", "candidate", 1),
            ],
            &[
                TOKEN,
                &binding.info_hash,
                "album",
                "owned.wav",
                CANDIDATE,
                "private-delayed-signature",
            ],
        );
    })
    .await
    .expect("the owned real-clock delayed workflow exceeded its total test deadline");
}

#[tokio::test]
async fn cancelled_and_failed_polls_rearm_the_owned_receipt_without_reunlocking() {
    const TOKEN: &str = "owned-private-delayed-cancellation-key";
    tokio::time::timeout(Duration::from_secs(25), async {
        let binding = MagnetFileBinding {
            magnet_id: 7123,
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            magnet_bytes: 1200,
            file_path: vec!["album".into(), "owned.wav".into()],
            file_bytes: 400,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let received = Arc::new(Notify::new());
        let fixture = Fixture::start(vec![
            Reply::Json(
                serde_json::to_vec(&json!({
                    "status": "success",
                    "data": { "magnets": [{ "id": 7123, "size": 1200, "statusCode": 4 }] }
                }))
                .unwrap(),
            ),
            Reply::Json(files()),
            Reply::Json(
                serde_json::to_vec(&json!({
                    "status": "success", "data": { "delayed": 2468, "filesize": 400 }
                }))
                .unwrap(),
            ),
            Reply::HoldUntilDisconnect(received.clone()),
            Reply::DisconnectAfterRequest,
            Reply::Json(
                serde_json::to_vec(&json!({
                    "status": "success", "data": { "status": 1, "time_left": 2 }
                }))
                .unwrap(),
            ),
        ])
        .await;
        let client =
            AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
                .expect("the owned TLS client must initialize");
        let capture = Capture::default();
        let subscriber = capture.dispatch();
        let PreparedFile::Delayed(pending) = client
            .prepare_bound_file(&binding)
            .with_subscriber(subscriber.clone())
            .await
            .expect("the exact file must produce one delayed ACK")
        else {
            panic!("the owned delayed acknowledgment must remain delayed")
        };
        let stored = serde_json::to_vec(&pending).unwrap();
        let mut pending: DelayedFileReceipt = serde_json::from_slice(&stored).unwrap();

        let cancelled_at;
        {
            let mut held = Box::pin(
                client
                    .poll_delayed(&mut pending)
                    .with_subscriber(subscriber.clone()),
            );
            tokio::time::timeout(Duration::from_secs(8), async {
                tokio::select! {
                    _ = received.notified() => {},
                    _ = &mut held => panic!("the provider must hold the actual in-flight poll"),
                }
            })
            .await
            .expect("the actual delayed poll did not reach the owned provider");
            cancelled_at = Instant::now();
            tracing::dispatcher::with_default(&subscriber, || drop(held));
        }
        let failed = client
            .poll_delayed(&mut pending)
            .with_subscriber(subscriber.clone())
            .await;
        let recovered = client
            .poll_delayed(&mut pending)
            .with_subscriber(subscriber)
            .await;
        let requests = fixture.close().await;

        assert!(matches!(failed, Err(Error::Transport)));
        match recovered.expect("the same owned receipt must recover its read capacity") {
            DelayedPoll::Processing {
                poll_after_seconds,
                time_left_seconds,
            } => {
                assert_eq!(poll_after_seconds, 5);
                assert_eq!(time_left_seconds, 2);
            }
            DelayedPoll::Candidate(_) => panic!("the provider did not report completion"),
        }
        assert!(pending.binding() == &binding);
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        assert_eq!(requests.len(), 6);
        assert!(
            requests[3]
                .received_at
                .duration_since(requests[2].received_at)
                >= Duration::from_secs(5)
        );
        assert!(requests[4].received_at.duration_since(cancelled_at) >= Duration::from_secs(5));
        assert!(
            requests[4]
                .received_at
                .duration_since(requests[3].received_at)
                >= Duration::from_secs(5)
        );
        assert!(
            requests[5]
                .received_at
                .duration_since(requests[4].received_at)
                >= Duration::from_secs(5)
        );
        for (index, request) in requests.iter().enumerate() {
            assert!(request.method == "POST");
            assert!(request.server_name == "api.alldebrid.com");
            assert!(request.header("host") == Some("api.alldebrid.com"));
            assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            assert!(request.header("content-type") == Some("application/x-www-form-urlencoded"));
            assert!(request.header("accept-encoding") == Some("identity"));
            assert!(request.header("cookie").is_none());
            let (target, body) = match index {
                0 => ("/v4.1/magnet/status", "id=7123"),
                1 => ("/v4/magnet/files", "id%5B%5D=7123"),
                2 => (
                    "/v4/link/unlock",
                    "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-selected",
                ),
                3..=5 => ("/v4/link/delayed", "id=2468"),
                _ => unreachable!(),
            };
            assert!(request.target == target);
            assert!(request.body == body.as_bytes());
        }
        capture.assert_workflows(
            &[
                ("bound_file_preparation", "delayed", 1),
                ("delayed_file_poll", "cancelled", 0),
                ("delayed_file_poll", "transport", 0),
                ("delayed_file_poll", "processing", 1),
            ],
            &[
                TOKEN,
                &binding.info_hash,
                "album",
                "owned.wav",
                "owned-selected",
            ],
        );
    })
    .await
    .expect("the bounded real-clock cancellation/recovery workflow exceeded its deadline");
}

fn files() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "success",
        "data": { "magnets": [{
            "id": "7123",
            "files": [
                { "n": "other", "e": [{ "n": "owned.wav", "s": 400, "l": "https://alldebrid.com/f/owned-decoy" }] },
                { "n": "album", "e": [{ "n": "owned.wav", "s": 400, "l": "https://alldebrid.com/f/owned-selected" }] },
                { "n": "readme.txt", "s": 400, "l": "https://alldebrid.com/f/owned-note" }
            ]
        }] }
    })).unwrap()
}
