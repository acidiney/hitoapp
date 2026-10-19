mod support;

use hito_debrid_all_debrid::{AllDebridClient, Error, MagnetFileBinding, MagnetPhase};
use serde_json::json;
use support::{Fixture, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-provider-key-kept-private";

#[tokio::test]
async fn configured_nested_file_retains_its_binding_through_processing_completion_and_change() {
    let configured = configured_binding();
    let persisted =
        serde_json::to_vec(&configured).expect("the configured snapshot must serialize");
    let binding: MagnetFileBinding =
        serde_json::from_slice(&persisted).expect("the configured snapshot must reopen");
    let fixture = Fixture::start(vec![
        status(1, Some(300)),
        status(4, None),
        files("owned.wav"),
        status(4, None),
        files("replacement.wav"),
    ])
    .await;
    let client = AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let subscriber = capture.dispatch();
    let processing = client
        .observe(&binding)
        .with_subscriber(subscriber.clone())
        .await;
    let complete = client
        .observe(&binding)
        .with_subscriber(subscriber.clone())
        .await;
    let changed = client.observe(&binding).with_subscriber(subscriber).await;
    let requests = fixture.close().await;
    capture.assert_workflows(
        &[
            ("magnet_observation", "processing", 1),
            ("magnet_observation", "provider_completed", 1),
            ("magnet_observation", "source_changed", 0),
        ],
        &[
            TOKEN,
            &binding.info_hash,
            "owned private bundle",
            "album",
            "owned.wav",
            "replacement.wav",
            "readme.txt",
        ],
    );

    let processing = processing.expect("the configured magnet must report current processing");
    assert!(processing.binding == configured);
    assert_eq!(processing.phase, MagnetPhase::Processing);
    assert_eq!(processing.progress_basis_points, Some(2500));
    let complete = complete.expect("the exact nested file must report provider completion");
    assert!(complete.binding == configured);
    assert_eq!(complete.phase, MagnetPhase::ProviderCompleted);
    assert_eq!(complete.progress_basis_points, Some(10_000));
    assert!(matches!(changed, Err(Error::SourceChanged)));
    assert!(
        binding == configured,
        "observation must preserve the configured witness"
    );
    assert_eq!(requests.len(), 5);
    for (index, request) in requests.iter().enumerate() {
        let status = matches!(index, 0 | 1 | 3);
        assert!(request.server_name == "api.alldebrid.com");
        assert!(request.method == "POST");
        assert!(
            request.target
                == if status {
                    "/v4.1/magnet/status"
                } else {
                    "/v4/magnet/files"
                }
        );
        assert!(request.header("host") == Some("api.alldebrid.com"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("content-type") == Some("application/x-www-form-urlencoded"));
        assert!(request.header("accept-encoding") == Some("identity"));
        assert!(request.header("cookie").is_none());
        assert!(
            request.body
                == if status {
                    b"id=7123".as_slice()
                } else {
                    b"id%5B%5D=7123".as_slice()
                }
        );
    }
}

#[tokio::test]
async fn duplicate_nested_paths_are_rejected_without_poisoning_the_configured_binding() {
    let binding = configured_binding();
    let before = serde_json::to_vec(&binding).expect("the configured snapshot must serialize");
    let mut ambiguous: serde_json::Value =
        serde_json::from_slice(&files("owned.wav")).expect("the owned file tree must deserialize");
    let roots = ambiguous["data"]["magnets"][0]["files"]
        .as_array_mut()
        .expect("the owned file tree must contain roots");
    roots.pop();
    let children = roots[1]["e"]
        .as_array_mut()
        .expect("the owned album directory must contain children");
    let mut duplicate = children[0].clone();
    duplicate["l"] = "https://alldebrid.com/f/owned-ambiguous".into();
    children.push(duplicate);
    let fixture = Fixture::start(vec![
        status(4, None),
        serde_json::to_vec(&ambiguous).expect("the ambiguous provider tree must serialize"),
        status(4, None),
        files("owned.wav"),
    ])
    .await;
    let client = AllDebridClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let subscriber = capture.dispatch();
    let rejected = client
        .observe(&binding)
        .with_subscriber(subscriber.clone())
        .await;
    let recovered = client.observe(&binding).with_subscriber(subscriber).await;
    let requests = fixture.close().await;
    capture.assert_workflows(
        &[
            ("magnet_observation", "invalid_response", 0),
            ("magnet_observation", "provider_completed", 1),
        ],
        &[
            TOKEN,
            &binding.info_hash,
            "owned private bundle",
            "album",
            "owned.wav",
            "readme.txt",
        ],
    );

    assert!(matches!(rejected, Err(Error::InvalidResponse)));
    let recovered = recovered.expect("a valid provider tree must recover without rebinding");
    assert!(recovered.binding == binding);
    assert_eq!(recovered.phase, MagnetPhase::ProviderCompleted);
    assert_eq!(recovered.progress_basis_points, Some(10_000));
    assert!(serde_json::to_vec(&binding).unwrap() == before);
    assert_eq!(requests.len(), 4);
    for (index, request) in requests.iter().enumerate() {
        let status = index % 2 == 0;
        assert!(request.server_name == "api.alldebrid.com");
        assert!(request.method == "POST");
        assert!(request.header("host") == Some("api.alldebrid.com"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("cookie").is_none());
        assert!(
            request.target
                == if status {
                    "/v4.1/magnet/status"
                } else {
                    "/v4/magnet/files"
                }
        );
        assert!(
            request.body
                == if status {
                    b"id=7123".as_slice()
                } else {
                    b"id%5B%5D=7123".as_slice()
                }
        );
    }
}

fn configured_binding() -> MagnetFileBinding {
    MagnetFileBinding {
        magnet_id: 7123,
        info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
        magnet_bytes: 1200,
        file_path: vec!["album".into(), "owned.wav".into()],
        file_bytes: 400,
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
