#[path = "observation/configuration.rs"]
mod configuration;
#[path = "observation/identity.rs"]
mod identity;
#[path = "observation/lifecycle.rs"]
mod lifecycle;
#[path = "observation/rejections.rs"]
mod rejections;
#[path = "observation/response.rs"]
mod response;
mod support;

use hito_debrid_torbox::{Error, TorBoxClient, TorrentFileBinding, TorrentPhase};
use serde_json::json;
use support::{Fixture, logs::Capture};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-torbox-provider-key-kept-private";

#[tokio::test]
async fn exact_file_binding_requires_both_completion_flags_and_rejects_a_changed_selected_file() {
    let configured = configured_binding();
    let persisted = serde_json::to_vec(&configured).expect("the trusted binding must serialize");
    let binding: TorrentFileBinding =
        serde_json::from_slice(&persisted).expect("the trusted binding must reopen");
    let fixture = Fixture::start(vec![
        observation(false, true, "album/owned.wav"),
        observation(true, false, "album/owned.wav"),
        observation(true, true, "album/owned.wav"),
        observation(true, true, "album/replacement.wav"),
    ])
    .await;
    let client = TorBoxClient::with_test_tls(TOKEN.into(), fixture.ca_der(), fixture.address())
        .expect("the owned TLS client must initialize");
    let capture = Capture::default();
    let subscriber = capture.dispatch();
    let unfinished = client
        .observe(&binding)
        .with_subscriber(subscriber.clone())
        .await;
    let absent = client
        .observe(&binding)
        .with_subscriber(subscriber.clone())
        .await;
    let complete = client
        .observe(&binding)
        .with_subscriber(subscriber.clone())
        .await;
    let changed = client.observe(&binding).with_subscriber(subscriber).await;
    let requests = fixture.close().await;

    let unfinished = unfinished.expect("a matching unfinished file must remain incomplete");
    assert!(unfinished.binding == configured);
    assert_eq!(unfinished.phase, TorrentPhase::Incomplete);
    let absent = absent.expect("a matching absent file must remain incomplete");
    assert!(absent.binding == configured);
    assert_eq!(absent.phase, TorrentPhase::Incomplete);
    let complete = complete.expect("both provider flags must qualify only completion metadata");
    assert!(complete.binding == configured);
    assert_eq!(complete.phase, TorrentPhase::ProviderCompleted);
    assert!(matches!(changed, Err(Error::SourceChanged)));
    assert!(
        binding == configured,
        "observation must preserve the trusted binding"
    );
    assert!(serde_json::to_vec(&binding).unwrap() == persisted);

    assert_eq!(requests.len(), 4);
    for request in requests {
        assert!(request.server_name == "api.torbox.app");
        assert!(request.method == "GET");
        assert!(request.target == "/v1/api/torrents/mylist?id=7123&bypass_cache=true");
        assert!(request.header("host") == Some("api.torbox.app"));
        assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
        assert!(request.header("accept") == Some("application/json"));
        assert!(request.header("accept-encoding") == Some("identity"));
        assert!(request.header("content-type").is_none());
        assert!(request.header("cookie").is_none());
        assert!(request.body.is_empty());
    }
    capture.assert_observations(
        &[
            ("incomplete", 1),
            ("incomplete", 1),
            ("provider_completed", 1),
            ("source_changed", 0),
        ],
        &[
            TOKEN,
            &binding.info_hash,
            &binding.file_name,
            "album/replacement.wav",
            "owned private torrent",
            "owned private response detail",
            "owned private storage location",
        ],
    );
}

fn configured_binding() -> TorrentFileBinding {
    TorrentFileBinding {
        torrent_id: 7123,
        info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
        torrent_bytes: 1200,
        file_id: 7,
        file_name: "album/owned.wav".into(),
        file_bytes: 400,
    }
}

fn observation(finished: bool, present: bool, selected_name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "success": true,
        "error": null,
        "detail": "owned private response detail",
        "data": {
            "id": 7123,
            "hash": "0123456789abcdef0123456789abcdef01234567",
            "name": "owned private torrent",
            "size": 1200,
            "download_state": "completed",
            "download_finished": finished,
            "download_present": present,
            "files": [
                { "id": 8, "name": "album/owned.wav", "size": 400 },
                { "id": 7, "name": selected_name, "size": 400 }
            ],
            "download_path": "owned private storage location",
            "future_provider_field": "compatible extension"
        }
    }))
    .expect("the owned provider observation must serialize")
}
