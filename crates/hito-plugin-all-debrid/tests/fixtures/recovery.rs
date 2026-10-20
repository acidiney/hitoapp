#[path = "held_media.rs"]
mod held_media;

use super::{
    ARTIST, COOKIE, DIRECTORY, FILE, HANDLE_KEY, HASH, MEDIA_HOST, PROVIDER_TOKEN, SERVICE_TOKEN,
    TITLE, audio, http, logging, private_json, recording, support,
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetFileBinding, MediaAllowlist};
use hito_plugin_all_debrid::{Config, CuratedWavRecording, router};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const SOURCES: [&str; 3] = [
    "recovery-source-a",
    "recovery-source-b",
    "recovery-source-c",
];
const RECORDINGS: [&str; 3] = [
    "recovery-recording-a",
    "recovery-recording-b",
    "recovery-recording-c",
];
const TARGETS: [&str; 3] = [
    "/owned-private-recovery-a.wav?signature=owned-private-recovery-a-signature",
    "/owned-private-recovery-b.wav?signature=owned-private-recovery-b-signature",
    "/owned-private-recovery-c.wav?signature=owned-private-recovery-c-signature",
];
const LINKS: [&str; 3] = [
    "https://alldebrid.com/f/owned-private-recovery-a",
    "https://alldebrid.com/f/owned-private-recovery-b",
    "https://alldebrid.com/f/owned-private-recovery-c",
];

#[tokio::test]
async fn capacity_denial_preserves_the_delayed_receipt_for_read_only_recovery() {
    tokio::time::timeout(Duration::from_secs(35), async {
        let wave = audio::wav();
        let media = held_media::HeldMedia::start(wave.clone(), TARGETS.map(str::to_owned)).await;
        let bindings: [MagnetFileBinding; 3] = std::array::from_fn(|index| MagnetFileBinding {
            magnet_id: 7123 + index as u64,
            info_hash: HASH.into(),
            magnet_bytes: audio::WAV_BYTES as u64 * 2 + 400,
            file_path: vec![DIRECTORY.into(), FILE.into()],
            file_bytes: audio::WAV_BYTES as u64,
        });
        let original = serde_json::to_vec(&bindings).unwrap();
        let provider = support::Fixture::start(provider_replies(&bindings)).await;
        let capture = logging::Capture::default();
        let client = AllDebridClient::with_test_tls_media(
            PROVIDER_TOKEN.into(), provider.ca_der(), provider.address(), media.ca_der(), media.address(),
        ).unwrap();
        let recordings = bindings.iter().enumerate().map(|(index, binding)| {
            let mut metadata = recording();
            metadata.id = RECORDINGS[index].into();
            CuratedWavRecording {
                recording: metadata,
                source_id: SOURCES[index].into(),
                label: "Curated canonical PCM16 WAV".into(),
                binding: binding.clone(),
            }
        }).collect();
        let plugin = http::Server::start(router(Config {
            service_bearer: SERVICE_TOKEN.into(), receipt_key: HANDLE_KEY, client,
            media_origins: MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            recordings,
        }).unwrap(), capture.dispatch()).await;
        let client = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never())
            .timeout(Duration::from_secs(14)).build().unwrap();
        let mut receipts = Vec::new();
        for index in 0..3 {
            receipts.push(submit(&client, &plugin, index).await);
        }
        for receipt in &receipts {
            assert!(matches!(status(&client, &plugin, receipt).await, PreparationStatus::Ready {}));
        }

        let mut first = open(&client, &plugin, 0).await;
        assert_audio_headers(&first);
        assert_held_prefix(&mut first, &wave).await;
        media.wait_for_headers(0).await;
        let mut second = open(&client, &plugin, 1).await;
        assert_audio_headers(&second);
        assert_held_prefix(&mut second, &wave).await;
        media.wait_for_headers(1).await;
        assert_eq!(media.active(), 2);

        let denied = open(&client, &plugin, 2).await;
        assert_eq!(denied.status(), StatusCode::SERVICE_UNAVAILABLE);
        private_json(&denied.bytes().await.unwrap());
        assert_eq!(media.active(), 2);
        let recovered = status(&client, &plugin, &receipts[2]).await;
        let ready = matches!(recovered, PreparationStatus::Ready {});
        drop(first);
        media.wait_for_eof(0).await;
        assert_eq!(media.active(), 1);
        if ready {
            let mut response = open(&client, &plugin, 2).await;
            assert_audio_headers(&response);
            let mut received = Vec::new();
            while let Some(chunk) = response.chunk().await.unwrap() {
                assert!(chunk.len() <= audio::WAV_BYTES.saturating_sub(received.len()));
                received.extend_from_slice(&chunk);
            }
            assert!(received == wave);
            audio::assert_decoded(received);
        }
        drop(second);
        media.wait_for_eof(1).await;
        assert_eq!(media.active(), 0);
        plugin.close().await.unwrap();
        let requests = provider.close().await;
        let media_requests = media.close().await;
        let unlocks = requests.iter().filter(|request| request.target == "/v4/link/unlock").count();
        println!("Owned recovery checkpoint: recovered={} provider_requests={} unlocks={unlocks} media_requests={}",
            usize::from(ready), requests.len(), media_requests.len());
        assert!(ready, "capacity denial must preserve the acknowledged delayed receipt and unchanged handle for read-only recovery");
        assert_eq!(requests.len(), 27);
        assert_eq!(unlocks, 3);
        assert_eq!(media_requests.len(), 3);
        assert!(serde_json::to_vec(&bindings).unwrap() == original);
        for (poll, unlock) in [(9, 2), (12, 5), (15, 8)] {
            assert!(requests[poll].received_at.duration_since(requests[unlock].received_at) >= Duration::from_secs(5));
        }
        assert!(requests[22].received_at.duration_since(requests[15].received_at) >= Duration::from_secs(5));
        for (index, request) in requests.iter().enumerate() {
            let (target, body) = expected_provider_request(index);
            assert!(request.method == "POST" && request.target == target && request.body == body.as_bytes());
            assert!(request.server_name == "api.alldebrid.com");
            assert_eq!(request.header("host"), Some("api.alldebrid.com"));
            assert_eq!(request.header("authorization"), Some(format!("Bearer {PROVIDER_TOKEN}").as_str()));
            assert_eq!(request.header("content-type"), Some("application/x-www-form-urlencoded"));
            assert_eq!(request.header("accept-encoding"), Some("identity"));
            for name in ["cookie", "referer", "range", "proxy-authorization", "x-hito-plugin-protocol"] {
                assert!(request.header(name).is_none());
            }
        }
        for (index, request) in media_requests.iter().enumerate() {
            assert!(request.method == "GET" && request.target == TARGETS[index]
                && request.server_name == MEDIA_HOST && request.body.is_empty());
            assert_eq!(request.header("host"), Some(MEDIA_HOST));
            assert_eq!(request.header("accept-encoding"), Some("identity"));
            for name in ["authorization", "cookie", "referer", "range", "proxy-authorization", "x-hito-plugin-protocol"] {
                assert!(request.header(name).is_none());
            }
        }
        let mut private = vec![PROVIDER_TOKEN, SERVICE_TOKEN, COOKIE, TITLE, ARTIST, HASH, DIRECTORY, FILE];
        private.extend(LINKS);
        private.extend(TARGETS);
        private.extend(receipts.iter().map(|receipt| receipt.handle.as_str()));
        capture.assert_recovery_receipt(&private, audio::WAV_BYTES as u64);
    }).await.expect("the bounded delayed-receipt recovery workflow exceeded its deadline");
}

async fn submit(
    client: &reqwest::Client,
    plugin: &http::Server,
    index: usize,
) -> PreparationStatusRequest {
    let request = PreparationRequest {
        operation_id: Uuid::new_v4(),
        recording_id: RECORDINGS[index].into(),
        source_id: SOURCES[index].into(),
    };
    let (code, bytes) = http::json(
        client,
        plugin,
        Method::POST,
        "v1/sources/prepare",
        Some(SERVICE_TOKEN),
        Some(serde_json::to_value(&request).unwrap()),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    private_json(&bytes);
    let response: PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
    response.validate(&request).unwrap();
    let PreparationSubmission::Accepted { handle, status } = response.submission else {
        panic!("each exact delayed acknowledgment must retain its owned receipt")
    };
    assert!(matches!(
        status,
        PreparationStatus::Preparing {
            poll_after_seconds: 5,
            ..
        }
    ));
    PreparationStatusRequest {
        operation_id: request.operation_id,
        recording_id: request.recording_id,
        source_id: request.source_id,
        handle,
    }
}

async fn status(
    client: &reqwest::Client,
    plugin: &http::Server,
    request: &PreparationStatusRequest,
) -> PreparationStatus {
    let (code, bytes) = http::json(
        client,
        plugin,
        Method::POST,
        "v1/sources/preparation/status",
        Some(SERVICE_TOKEN),
        Some(serde_json::to_value(request).unwrap()),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    private_json(&bytes);
    let response: PreparationStatusResponse = serde_json::from_slice(&bytes).unwrap();
    response.validate(request).unwrap();
    assert!(response.handle == request.handle);
    response.status
}

async fn open(client: &reqwest::Client, plugin: &http::Server, index: usize) -> reqwest::Response {
    let mut target = plugin.base.join("v1/audio").unwrap();
    target
        .query_pairs_mut()
        .append_pair("source_id", SOURCES[index]);
    client
        .get(target)
        .bearer_auth(SERVICE_TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", COOKIE)
        .send()
        .await
        .unwrap()
}

fn assert_audio_headers(response: &reqwest::Response) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(
        response.headers()["content-length"],
        audio::WAV_BYTES.to_string()
    );
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    for name in [
        "location",
        "set-cookie",
        "content-disposition",
        "etag",
        "content-range",
        "accept-ranges",
    ] {
        assert!(response.headers().get(name).is_none());
    }
}

async fn assert_held_prefix(response: &mut reqwest::Response, wave: &[u8]) {
    let mut received = Vec::new();
    while received.len() < 44 {
        let chunk = response.chunk().await.unwrap().unwrap();
        assert!(chunk.len() <= 44 - received.len());
        received.extend_from_slice(&chunk);
    }
    assert!(received == wave[..44]);
}

fn provider_replies(bindings: &[MagnetFileBinding; 3]) -> Vec<Vec<u8>> {
    let mut replies = Vec::new();
    for (index, binding) in bindings.iter().enumerate() {
        replies.extend(observation(binding, LINKS[index]));
        replies.push(
            json!({"status":"success","data":{"delayed":2468+index,"filesize":binding.file_bytes}}),
        );
    }
    for (index, binding) in bindings.iter().enumerate() {
        replies.push(candidate(index));
        replies.extend(observation(binding, LINKS[index]));
    }
    for (binding, link) in bindings.iter().zip(LINKS).take(2) {
        replies.extend(observation(binding, link));
    }
    replies.push(candidate(2));
    replies.extend(observation(&bindings[2], LINKS[2]));
    replies.extend(observation(&bindings[2], LINKS[2]));
    assert_eq!(replies.len(), 27);
    replies
        .into_iter()
        .map(|value| serde_json::to_vec(&value).unwrap())
        .collect()
}

fn candidate(index: usize) -> Value {
    json!({"status":"success","data":{"status":2,"time_left":0,
        "link":format!("https://{MEDIA_HOST}{}", TARGETS[index])}})
}

fn observation(binding: &MagnetFileBinding, link: &str) -> [Value; 2] {
    [
        json!({"status":"success","data":{"magnets":[{
            "id":binding.magnet_id,"size":binding.magnet_bytes,"statusCode":4
        }]}}),
        json!({"status":"success","data":{"magnets":[{
            "id":binding.magnet_id.to_string(),"files":[
                {"n":DIRECTORY,"e":[{"n":FILE,"s":binding.file_bytes,"l":link}]},
                {"n":"other","e":[{"n":FILE,"s":binding.file_bytes,
                    "l":"https://alldebrid.com/f/owned-private-recovery-decoy"}]},
                {"n":"readme.txt","s":400,"l":"https://alldebrid.com/f/owned-private-recovery-note"}
            ]
        }]}}),
    ]
}

fn expected_provider_request(index: usize) -> (&'static str, String) {
    let (binding, resource) = match index {
        0..=8 => (index / 3, index % 3),
        9 => (0, 3),
        10 => (0, 0),
        11 => (0, 1),
        12 => (1, 3),
        13 => (1, 0),
        14 => (1, 1),
        15 | 22 => (2, 3),
        16 | 23 | 25 => (2, 0),
        17 | 24 | 26 => (2, 1),
        18 => (0, 0),
        19 => (0, 1),
        20 => (1, 0),
        21 => (1, 1),
        _ => unreachable!(),
    };
    match resource {
        0 => ("/v4.1/magnet/status", format!("id={}", 7123 + binding)),
        1 => ("/v4/magnet/files", format!("id%5B%5D={}", 7123 + binding)),
        2 => (
            "/v4/link/unlock",
            format!(
                "link={}",
                url::form_urlencoded::byte_serialize(LINKS[binding].as_bytes()).collect::<String>()
            ),
        ),
        3 => ("/v4/link/delayed", format!("id={}", 2468 + binding)),
        _ => unreachable!(),
    }
}
