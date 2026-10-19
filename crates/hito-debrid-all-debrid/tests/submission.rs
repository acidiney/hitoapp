mod support;

use hito_debrid_all_debrid::{
    AllDebridClient, Error, MagnetFileBinding, MagnetFileSelection, MagnetPhase,
    MagnetUploadReceipt,
};
use serde_json::json;
use support::{Fixture, Reply, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-private-upload-key";
const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

#[tokio::test]
async fn acknowledged_upload_reopens_its_exact_file_binding_without_resubmission() {
    let selection = MagnetFileSelection {
        info_hash: HASH.into(),
        magnet_bytes: 1200,
        file_path: vec!["album".into(), "owned.wav".into()],
        file_bytes: 400,
    };
    let fixture = Fixture::start(vec![
        serde_json::to_vec(&json!({
            "status": "success",
            "data": { "magnets": [{
                "magnet": HASH, "hash": HASH, "id": 7123,
                "size": 1200, "ready": false,
                "name": "owned-private-upload-title"
            }] }
        }))
        .unwrap(),
        status(1, Some(300)),
        status(4, None),
        files("owned.wav"),
    ])
    .await;
    let client = AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let subscriber = capture.dispatch();
    let receipt = client
        .submit_magnet(&selection)
        .with_subscriber(subscriber.clone())
        .await
        .expect("one exact upload ACK must retain the requested hash and file selection");
    assert_eq!(receipt.binding().magnet_id, 7123);
    assert!(receipt.binding().info_hash == HASH);
    assert_eq!(receipt.binding().magnet_bytes, 1200);
    assert!(receipt.binding().file_path == ["album", "owned.wav"]);
    assert_eq!(receipt.binding().file_bytes, 400);
    let persisted = serde_json::to_vec(&receipt).expect("the owned receipt must serialize");
    drop(client);
    let reopened: MagnetUploadReceipt =
        serde_json::from_slice(&persisted).expect("the trusted owned receipt snapshot must reopen");
    assert!(reopened.binding() == receipt.binding());
    let client = AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the recreated owned TLS client must initialize");
    let processing = client
        .observe(reopened.binding())
        .with_subscriber(subscriber.clone())
        .await;
    let complete = client
        .observe(reopened.binding())
        .with_subscriber(subscriber)
        .await;
    let requests = fixture.close().await;
    capture.assert_workflows(
        &[
            ("magnet_submission", "acknowledged", 1),
            ("magnet_observation", "processing", 1),
            ("magnet_observation", "provider_completed", 1),
        ],
        &[
            TOKEN,
            HASH,
            "owned-private-upload-title",
            "owned private bundle",
            "album",
            "owned.wav",
            "readme.txt",
        ],
    );

    let processing = processing.expect("the acknowledged magnet must report current processing");
    assert!(processing.binding == *reopened.binding());
    assert_eq!(processing.phase, MagnetPhase::Processing);
    assert_eq!(processing.progress_basis_points, Some(2500));
    let complete = complete.expect("the exact nested file must report provider completion");
    assert!(complete.binding == *reopened.binding());
    assert_eq!(complete.phase, MagnetPhase::ProviderCompleted);
    assert_eq!(complete.progress_basis_points, Some(10_000));
    assert_eq!(requests.len(), 4);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "POST");
        assert!(request.server_name == "api.alldebrid.com");
        assert!(request.header("host") == Some("api.alldebrid.com"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("cookie").is_none());
        assert!(request.header("accept-encoding") == Some("identity"));
        let (target, body) = match index {
            0 => ("/v4/magnet/upload", format!("magnets%5B%5D={HASH}")),
            1 | 2 => ("/v4.1/magnet/status", "id=7123".into()),
            3 => ("/v4/magnet/files", "id%5B%5D=7123".into()),
            _ => unreachable!(),
        };
        assert!(request.target == target);
        assert!(request.body == body.as_bytes());
    }
}

#[tokio::test]
async fn received_upload_without_acknowledgment_is_unknown_and_never_retried() {
    let selection = MagnetFileSelection {
        info_hash: HASH.into(),
        magnet_bytes: 1200,
        file_path: vec!["album".into(), "owned.wav".into()],
        file_bytes: 400,
    };
    let prior = MagnetFileBinding {
        magnet_id: 9951,
        info_hash: "ffffffffffffffffffffffffffffffffffffffff".into(),
        magnet_bytes: 1200,
        file_path: vec!["preserved".into(), "existing.wav".into()],
        file_bytes: 400,
    };
    let before = serde_json::to_vec(&prior).unwrap();
    let fixture = Fixture::start(vec![
        Reply::DisconnectAfterRequest,
        Reply::Json(
            serde_json::to_vec(&json!({
                "status": "success",
                "data": { "magnets": [{
                    "id": 9951, "size": 1200, "statusCode": 1, "downloaded": 300
                }] }
            }))
            .unwrap(),
        ),
    ])
    .await;
    let client = AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let subscriber = capture.dispatch();
    let submitted = client
        .submit_magnet(&selection)
        .with_subscriber(subscriber.clone())
        .await;
    let observed = client.observe(&prior).with_subscriber(subscriber).await;
    let requests = fixture.close().await;
    capture.assert_workflows(
        &[
            ("magnet_submission", "submission_unknown", 0),
            ("magnet_observation", "processing", 1),
        ],
        &[
            TOKEN,
            HASH,
            &prior.info_hash,
            "album",
            "owned.wav",
            "preserved",
            "existing.wav",
        ],
    );

    let error = submitted
        .err()
        .expect("a lost ACK must not fabricate an upload receipt");
    assert_eq!(error, Error::SubmissionUnknown);
    assert_eq!(
        error.to_string(),
        "The AllDebrid upload acknowledgment is uncertain"
    );
    assert_eq!(format!("{error:?}"), "SubmissionUnknown");
    assert!(selection.info_hash == HASH);
    assert_eq!(selection.magnet_bytes, 1200);
    assert!(selection.file_path == ["album", "owned.wav"]);
    assert_eq!(selection.file_bytes, 400);
    assert!(serde_json::to_vec(&prior).unwrap() == before);
    let observed = observed.expect("a later explicit independent read must retain client capacity");
    assert!(observed.binding == prior);
    assert_eq!(observed.phase, MagnetPhase::Processing);
    assert_eq!(observed.progress_basis_points, Some(2500));
    assert_eq!(requests.len(), 2);
    assert!(requests[0].target == "/v4/magnet/upload");
    assert!(requests[0].body == format!("magnets%5B%5D={HASH}").as_bytes());
    assert!(requests[1].target == "/v4.1/magnet/status");
    assert!(requests[1].body == b"id=9951");
    for request in requests {
        assert!(request.method == "POST");
        assert!(request.server_name == "api.alldebrid.com");
        assert!(request.header("host") == Some("api.alldebrid.com"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("cookie").is_none());
        assert!(request.header("accept-encoding") == Some("identity"));
    }
}

fn status(code: u8, downloaded: Option<u64>) -> Vec<u8> {
    let mut magnet = json!({
        "id": 7123,
        "filename": "owned private bundle",
        "size": 1200,
        "statusCode": code,
        "future_provider_field": "compatible extension"
    });
    if let Some(downloaded) = downloaded {
        magnet["downloaded"] = downloaded.into();
    }
    serde_json::to_vec(&json!({ "status": "success", "data": { "magnets": [magnet] } }))
        .expect("the owned provider status must serialize")
}

fn files(selected_name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "success",
        "data": { "magnets": [{
            "id": "7123",
            "files": [
                { "n": "other", "e": [{ "n": "owned.wav", "s": 400, "l": "https://alldebrid.com/f/owned-decoy" }] },
                { "n": "album", "e": [{ "n": selected_name, "s": 400, "l": "https://alldebrid.com/f/owned-selected" }] },
                { "n": "readme.txt", "s": 400, "l": "https://alldebrid.com/f/owned-note" }
            ]
        }] }
    }))
    .expect("the owned provider file tree must serialize")
}
