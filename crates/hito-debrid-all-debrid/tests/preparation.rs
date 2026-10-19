mod support;

use hito_debrid_all_debrid::{AllDebridClient, DelayedFileReceipt, MagnetFileBinding};
use serde_json::json;
use support::{Fixture, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-private-preparation-key";

#[tokio::test]
async fn delayed_unlock_reobserves_the_exact_nested_file_without_polling_or_media() {
    let configured = MagnetFileBinding {
        magnet_id: 7123,
        info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
        magnet_bytes: 1200,
        file_path: vec!["album".into(), "owned.wav".into()],
        file_bytes: 400,
    };
    let before = serde_json::to_vec(&configured).unwrap();
    let binding: MagnetFileBinding = serde_json::from_slice(&before).unwrap();
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
    ])
    .await;
    let client = AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let prepared = client
        .prepare_bound_file(&binding)
        .with_subscriber(capture.dispatch())
        .await;
    let requests = fixture.close().await;
    capture.assert_workflows(
        &[("bound_file_preparation", "delayed", 1)],
        &[
            TOKEN,
            &binding.info_hash,
            "album",
            "owned.wav",
            "readme.txt",
        ],
    );

    let receipt = prepared.expect("a valid delayed ACK must retain the exact selected binding");
    assert!(receipt.binding() == &configured);
    assert_eq!(receipt.poll_after_seconds(), 5);
    let persisted = serde_json::to_vec(&receipt).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&persisted).unwrap(),
        json!({ "binding": configured, "delayed_id": 2468 })
    );
    let reopened: DelayedFileReceipt =
        serde_json::from_slice(&persisted).expect("the trusted owned delayed snapshot must reopen");
    assert!(reopened.binding() == &binding);
    assert_eq!(reopened.poll_after_seconds(), 5);
    assert!(serde_json::to_vec(&binding).unwrap() == before);
    assert_eq!(requests.len(), 3);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "POST");
        assert!(request.server_name == "api.alldebrid.com");
        assert!(request.header("host") == Some("api.alldebrid.com"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("content-type") == Some("application/x-www-form-urlencoded"));
        assert!(request.header("cookie").is_none());
        assert!(request.header("accept-encoding") == Some("identity"));
        let (target, body) = match index {
            0 => ("/v4.1/magnet/status", "id=7123"),
            1 => ("/v4/magnet/files", "id%5B%5D=7123"),
            2 => (
                "/v4/link/unlock",
                "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-selected",
            ),
            _ => unreachable!(),
        };
        assert!(request.target == target);
        assert!(request.body == body.as_bytes());
    }
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
