use super::{
    ARTIST, COOKIE, DIRECTORY, FILE, HANDLE_KEY, HASH, MEDIA_HOST, MEDIA_TARGET, PROVIDER_TOKEN,
    RECORDING, SELECTED_LINK, SERVICE_TOKEN, SOURCE, TITLE, WRONG_TOKEN, audio, http, logging,
    media, recording, support,
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetFileBinding, MediaAllowlist};
use hito_plugin_all_debrid::{Config, CuratedWavRecording, InitError, router};
use hito_plugin_sdk::{ProviderRecording, RecordingAttribution, RecordingRequest};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn recording_detail_admits_exact_transport_budget_and_rejects_one_more_byte() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let accepted = metadata(32_768);
        let oversized = metadata(32_769);
        let expected = serde_json::to_vec(&accepted).unwrap();
        let media = media::MediaFixture::start(audio::wav()).await;
        let provider = support::Fixture::start(Vec::<Vec<u8>>::new()).await;
        let client = AllDebridClient::with_test_tls_media(
            PROVIDER_TOKEN.into(),
            provider.ca_der(),
            provider.address(),
            media.ca_der(),
            media.address(),
        )
        .unwrap();
        let configure = |recording| Config {
            service_bearer: SERVICE_TOKEN.into(),
            receipt_key: HANDLE_KEY,
            client: client.clone(),
            media_origins: MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            recordings: vec![CuratedWavRecording {
                recording,
                source_id: SOURCE.into(),
                label: "Curated canonical PCM16 WAV".into(),
                binding: MagnetFileBinding {
                    magnet_id: 7123,
                    info_hash: HASH.into(),
                    magnet_bytes: audio::WAV_BYTES as u64 * 2 + 400,
                    file_path: vec![DIRECTORY.into(), FILE.into()],
                    file_bytes: audio::WAV_BYTES as u64,
                },
            }],
        };
        let accepted_router = router(configure(accepted.clone())).unwrap();
        let rejected_at_startup =
            matches!(router(configure(oversized)), Err(InitError::InvalidCatalog));
        let capture = logging::Capture::default();
        let plugin = http::Server::start(accepted_router, capture.dispatch()).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap();
        let request = RecordingRequest {
            recording_id: RECORDING.into(),
        };
        let (status, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/recordings/detail",
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(request).unwrap()),
        )
        .await;
        plugin.close().await.unwrap();
        let provider_requests = provider.close().await;
        let media_requests = media.close().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.len(), 32_768);
        assert!(body == expected);
        let actual: ProviderRecording = serde_json::from_slice(&body).unwrap();
        actual.validate().unwrap();
        assert!(actual == accepted);
        let private = [
            PROVIDER_TOKEN,
            SERVICE_TOKEN,
            WRONG_TOKEN,
            COOKIE,
            HASH,
            DIRECTORY,
            FILE,
            SELECTED_LINK,
            MEDIA_TARGET,
        ];
        assert_private(std::str::from_utf8(&body).unwrap(), &private);
        assert!(provider_requests.is_empty() && media_requests.is_empty());
        assert_detail_log(&capture, &private);
        assert!(
            rejected_at_startup,
            "one byte beyond the recording-detail transport budget must reject catalog startup"
        );
    })
    .await
    .expect("the owned detail-budget workflow exceeded its deadline");
}

fn metadata(bytes: usize) -> ProviderRecording {
    let mut value = recording();
    value.attribution = Some(RecordingAttribution {
        creators: (0..16)
            .map(|index| format!("owned-private-ad-detail-creator-{index:02}-"))
            .collect(),
        provider_label: "owned-private-ad-detail-provider-label".into(),
        content_url: "https://attribution.hito-fixture.invalid/recording".into(),
    });
    let mut remaining = bytes
        .checked_sub(serde_json::to_vec(&value).unwrap().len())
        .unwrap();
    for creator in &mut value.attribution.as_mut().unwrap().creators {
        let additional = remaining.min(2048 - creator.len());
        creator.push_str(&"x".repeat(additional));
        remaining -= additional;
    }
    assert_eq!(remaining, 0);
    value.validate().unwrap();
    assert_eq!(serde_json::to_vec(&value).unwrap().len(), bytes);
    value
}

fn assert_private(text: &str, private: &[&str]) {
    for value in private.iter().copied() {
        let escaped = serde_json::to_string(value).unwrap();
        assert!(!text.contains(value) && !text.contains(&escaped[1..escaped.len() - 1]));
    }
}

fn assert_detail_log(capture: &logging::Capture, private: &[&str]) {
    let text = capture.text();
    assert_private(&text, private);
    assert_private(
        &text,
        &[
            TITLE,
            ARTIST,
            RECORDING,
            SOURCE,
            "owned-private-ad-detail-creator-",
            "owned-private-ad-detail-provider-label",
            "http://",
            "https://",
            "http%3A",
            "https%3A",
            "Bearer ",
        ],
    );
    let events = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    let request_id = event["span"]["request_id"].as_str().unwrap();
    assert_eq!(Uuid::parse_str(request_id).unwrap().get_version_num(), 4);
    let elapsed = event["fields"]["elapsed_ms"].as_u64().unwrap();
    let span =
        json!({"name":"all_debrid_plugin_request","operation":"detail","request_id":request_id});
    assert!(
        event
            == &json!({
                "level":"DEBUG",
                "fields":{"event":"all_debrid_plugin_request_finished","outcome":"success","status":200,"elapsed_ms":elapsed},
                "target":"hito_plugin_all_debrid::http",
                "span":span,
                "spans":[span],
            }),
        "only the fixed safe correlated detail event may be logged"
    );
}
