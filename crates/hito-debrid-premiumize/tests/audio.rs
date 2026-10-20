#[path = "audio_fixture/mod.rs"]
mod fixture;

use fixture::{Fixture, Reply, logs::Capture};
use hito_audio::Decoder;
use hito_debrid_premiumize::{
    MediaAllowlist, PremiumizeClient, TransferBinding, TransferFileBinding,
};
use serde_json::json;
use std::{io::Cursor, time::Duration};
use tracing::instrument::WithSubscriber;

const TOKEN: &str = "owned-premiumize-audio-credential-kept-private";
const TRANSFER_ID: &str = "owned-exact-transfer-kept-private";
const ITEM_ID: &str = "owned-exact-item-kept-private";
const FOLDER_ID: &str = "owned-cloud-folder-kept-private";
const FILE_NAME: &str = "owned private exact audio file.wav";
const TRANSFER_CREATED: u64 = 1_700_000_000;
const ITEM_CREATED: u64 = TRANSFER_CREATED + 120;
const MEDIA_ORIGIN: &str = "https://media.hito-fixture.invalid";
const MEDIA_TARGET: &str = "/owned-audio?ticket=owned-private-media-ticket";
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAMES: u32 = 96_000;

#[tokio::test]
async fn curated_single_file_transfer_streams_exact_bound_audio_without_media_credentials() {
    let wave = owned_wave();
    let configured = TransferFileBinding {
        transfer: TransferBinding {
            transfer_id: TRANSFER_ID.into(),
            created_at_unix_seconds: TRANSFER_CREATED,
        },
        item_id: ITEM_ID.into(),
        item_created_at_unix_seconds: ITEM_CREATED,
        file_name: FILE_NAME.into(),
        file_bytes: wave.len() as u64,
        folder_id: FOLDER_ID.into(),
    };
    let snapshot = serde_json::to_vec(&configured).expect("the curated binding must serialize");
    let binding: TransferFileBinding = serde_json::from_slice(&snapshot)
        .expect("the curated binding must reopen without private media targets");
    assert_ne!(
        binding.item_created_at_unix_seconds,
        binding.transfer.created_at_unix_seconds
    );
    let api = Fixture::start(
        "www.premiumize.me",
        vec![
            Reply {
                content_type: "application/json",
                body: transfer_list(),
            },
            Reply {
                content_type: "application/json",
                body: item_details(wave.len() as u64),
            },
        ],
    )
    .await;
    let media = Fixture::start(
        "media.hito-fixture.invalid",
        vec![Reply {
            content_type: "audio/wav",
            body: wave.clone(),
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
    .expect("the owned separate TLS peers must configure");
    let allowlist = MediaAllowlist::new(vec![MEDIA_ORIGIN.into()])
        .expect("the trusted exact media origin must configure");
    let capture = Capture::default();
    let received = async {
        let mut opened = client
            .open_bound_audio(&binding, &allowlist)
            .await
            .expect("the exact curated file must open supported bound audio");
        assert!(opened.binding == configured);
        assert_eq!(serde_json::to_vec(&opened.binding).unwrap(), snapshot);
        assert_eq!(opened.length, wave.len() as u64);
        assert_eq!(opened.format.sample_rate, SAMPLE_RATE);
        assert_eq!(opened.format.channels, CHANNELS);
        assert_eq!(opened.format.frames, u64::from(FRAMES));
        let mut received = Vec::new();
        while let Some(chunk) = opened
            .next_chunk()
            .await
            .expect("the bound body must reach valid EOF")
        {
            assert!(!chunk.is_empty() && chunk.len() <= 65_536);
            assert!(received.len() + chunk.len() <= wave.len());
            received.extend_from_slice(&chunk);
        }
        assert!(opened.next_chunk().await.unwrap().is_none());
        assert!(opened.binding == configured);
        received
    }
    .with_subscriber(capture.dispatch())
    .await;
    assert!(binding == configured);
    assert_eq!(serde_json::to_vec(&binding).unwrap(), snapshot);
    assert!(received == wave);
    let mut decoder = Decoder::open(Cursor::new(received)).expect("the received PCM must decode");
    assert_eq!(decoder.format().sample_rate, SAMPLE_RATE);
    assert_eq!(decoder.format().channels, CHANNELS);
    assert_eq!(decoder.format().duration, Some(Duration::from_secs(2)));
    let mut samples = 0_usize;
    while let Some(block) = decoder.read().expect("the received PCM frames must decode") {
        for sample in block.samples {
            assert_eq!(*sample, f32::from(pcm_sample(samples)) / 32_768.0);
            samples += 1;
        }
    }
    assert_eq!(samples, FRAMES as usize * usize::from(CHANNELS));
    let api_requests = api.close().await;
    let media_requests = media.close().await;
    assert_eq!(api_requests.len(), 2);
    for (request, target) in api_requests.iter().zip([
        "/api/transfer/list".to_owned(),
        format!("/api/item/details?id={ITEM_ID}"),
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
    assert_eq!(media_requests.len(), 1);
    let request = &media_requests[0];
    assert!(request.server_name == "media.hito-fixture.invalid");
    assert!(request.method == "GET" && request.target == MEDIA_TARGET);
    assert!(request.header("host") == Some("media.hito-fixture.invalid"));
    assert!(request.header("accept") == Some("audio/wav"));
    assert!(request.header("accept-encoding") == Some("identity"));
    for forbidden in [
        "authorization",
        "cookie",
        "referer",
        "range",
        "proxy-authorization",
        "apikey",
        "pin",
        "x-hito-plugin-protocol",
    ] {
        assert!(request.header(forbidden).is_none());
    }
    capture.assert_complete(
        wave.len() as u64,
        &[
            TOKEN,
            TRANSFER_ID,
            ITEM_ID,
            FOLDER_ID,
            FILE_NAME,
            MEDIA_TARGET,
            "owned-same-name-decoy",
            "owned-decoy-item",
            "owned-private-media-ticket",
            "owned-private-provider-message",
            "owned-private-media-cookie",
            "owned-private-cookie-value",
            "owned-private-upstream-name.wav",
            "owned-private-source",
            "media.hito-fixture.invalid",
            "www.premiumize.me",
        ],
    );
}

fn transfer_list() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "success",
        "transfers": [
            { "id": "owned-same-name-decoy", "name": FILE_NAME,
              "created_at": TRANSFER_CREATED, "status": "finished", "progress": 1.0,
              "file_id": "owned-decoy-item", "folder_id": FOLDER_ID },
            { "id": TRANSFER_ID, "name": FILE_NAME,
              "created_at": TRANSFER_CREATED, "status": "finished", "progress": 1.0,
              "file_id": ITEM_ID, "folder_id": FOLDER_ID,
              "message": "owned-private-provider-message",
              "src": "https://www.premiumize.me/owned-private-source" }
        ]
    }))
    .unwrap()
}

fn item_details(bytes: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "status": "success", "id": ITEM_ID, "name": FILE_NAME, "size": bytes,
        "created_at": ITEM_CREATED, "folder_id": FOLDER_ID, "mime_type": "audio/wav",
        "link": format!("{MEDIA_ORIGIN}{MEDIA_TARGET}"),
        "duration": 880, "future_provider_field": "compatible extension"
    }))
    .unwrap()
}

fn pcm_sample(index: usize) -> i16 {
    ((index * 73 % 32_000) as i16) - 16_000
}

fn owned_wave() -> Vec<u8> {
    let data_bytes = FRAMES * u32::from(CHANNELS) * 2;
    let mut wave = Vec::with_capacity(data_bytes as usize + 44);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&CHANNELS.to_le_bytes());
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(SAMPLE_RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    wave.extend_from_slice(&(CHANNELS * 2).to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..FRAMES as usize * usize::from(CHANNELS) {
        wave.extend_from_slice(&pcm_sample(index).to_le_bytes());
    }
    wave
}

#[path = "audio_cases/mod.rs"]
mod cases;
