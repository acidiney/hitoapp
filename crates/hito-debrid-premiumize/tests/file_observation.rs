#![cfg(feature = "test-support")]

#[path = "file_observation_fixture/logs.rs"]
mod logs;
#[path = "audio_fixture/mod.rs"]
pub mod wire_fixture;

use hito_debrid_premiumize::{
    BoundFilePhase, PremiumizeClient, TransferBinding, TransferFileBinding,
};
use logs::Capture;
use serde_json::json;
use tracing::instrument::WithSubscriber;
use wire_fixture::{Fixture, Reply};

const TOKEN: &str = "owned-file-observation-credential-kept-private";
const TRANSFER: &str = "owned-file-observation-transfer-kept-private";
const ITEM: &str = "owned-file-observation-item-kept-private";
const FOLDER: &str = "owned-file-observation-folder-kept-private";
const NAME: &str = "owned private reviewed file.wav";
const TRANSFER_CREATED: u64 = 1_700_000_000;
const ITEM_CREATED: u64 = TRANSFER_CREATED + 120;
const BYTES: u64 = 384_044;
const LINK: &str = "https://media.hito-fixture.invalid/owned-private-observation-target?token=owned-private-link-a";

#[tokio::test]
async fn exact_completed_and_seeding_files_are_observed_without_opening_media() {
    let configured = TransferFileBinding {
        transfer: TransferBinding {
            transfer_id: TRANSFER.into(),
            created_at_unix_seconds: TRANSFER_CREATED,
        },
        item_id: ITEM.into(),
        item_created_at_unix_seconds: ITEM_CREATED,
        file_name: NAME.into(),
        file_bytes: BYTES,
        folder_id: FOLDER.into(),
    };
    let snapshot =
        serde_json::to_vec(&configured).expect("the curated file binding must serialize");
    let binding: TransferFileBinding = serde_json::from_slice(&snapshot)
        .expect("the curated file binding must retain its independent creation witnesses");
    assert_ne!(
        binding.item_created_at_unix_seconds,
        binding.transfer.created_at_unix_seconds
    );
    let capture = Capture::default();
    for (status, fraction, expected) in [
        ("finished", 1.0, BoundFilePhase::ProviderCompleted),
        ("seeding", 0.5, BoundFilePhase::ProviderSeeding),
    ] {
        let api = Fixture::start(
            "www.premiumize.me",
            vec![
                Reply {
                    content_type: "application/json",
                    body: transfer_list(status, fraction),
                },
                Reply {
                    content_type: "application/json",
                    body: item_details(),
                },
            ],
        )
        .await;
        let media = Fixture::start(
            "media.hito-fixture.invalid",
            vec![Reply {
                content_type: "audio/wav",
                body: vec![0; 44],
            }],
        )
        .await;
        let client = PremiumizeClient::with_test_tls_media(
            TOKEN.into(),
            api.ca_der(),
            api.address(),
            media.ca_der(),
            media.address(),
        )
        .expect("the separate owned API and media peers must configure");
        let observed = client
            .observe_bound_file(&binding)
            .with_subscriber(capture.dispatch())
            .await
            .expect("the exact available cloud file must have a metadata-only observation");
        assert!(observed.binding == configured);
        assert_eq!(serde_json::to_vec(&observed.binding).unwrap(), snapshot);
        assert_eq!(observed.phase, expected);
        assert_eq!(observed.progress.fraction(), fraction);
        assert!(binding == configured);
        assert_eq!(serde_json::to_vec(&binding).unwrap(), snapshot);
        assert!(!std::str::from_utf8(&snapshot).unwrap().contains(LINK));
        assert_eq!(media.request_count(), 0);
        let requests = api.close().await;
        assert_eq!(requests.len(), 2);
        for (request, target) in requests.iter().zip([
            "/api/transfer/list".to_owned(),
            format!("/api/item/details?id={ITEM}"),
        ]) {
            assert!(request.server_name == "www.premiumize.me");
            assert!(request.method == "GET" && request.target == target);
            assert!(request.header("host") == Some("www.premiumize.me"));
            assert!(request.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()));
            assert!(request.header("accept") == Some("application/json"));
            assert!(request.header("accept-encoding") == Some("identity"));
            for forbidden in [
                "cookie",
                "referer",
                "content-type",
                "apikey",
                "pin",
                "proxy-authorization",
            ] {
                assert!(request.header(forbidden).is_none());
            }
        }
        assert!(media.close().await.is_empty());
    }
    capture.assert_available(&[
        TOKEN,
        TRANSFER,
        ITEM,
        FOLDER,
        NAME,
        LINK,
        "owned-private-link-a",
        "owned-private-provider-observation-message",
        "owned-same-name-decoy",
        "owned-decoy-item",
        "owned-private-media-cookie",
        "owned-private-cookie-value",
        "owned-private-upstream-name.wav",
        "media.hito-fixture.invalid",
        "www.premiumize.me",
    ]);
}

fn transfer_list(status: &str, fraction: f64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "success",
        "transfers": [
            { "id": "owned-same-name-decoy", "name": NAME,
              "created_at": TRANSFER_CREATED, "status": "finished", "progress": 1.0,
              "file_id": "owned-decoy-item", "folder_id": FOLDER },
            { "id": TRANSFER, "name": NAME,
              "created_at": TRANSFER_CREATED, "status": status, "progress": fraction,
              "file_id": ITEM, "folder_id": FOLDER,
              "message": "owned-private-provider-observation-message" }
        ]
    }))
    .unwrap()
}

fn item_details() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "success", "id": ITEM, "name": NAME, "size": BYTES,
        "created_at": ITEM_CREATED, "folder_id": FOLDER, "link": LINK,
        "future_provider_field": "owned-private-provider-observation-message"
    }))
    .unwrap()
}
