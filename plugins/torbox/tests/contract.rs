pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_testkit::plugin_server as http;
#[path = "support/logs.rs"]
mod logs;
pub use hito_debrid_torbox::test_support::media_fixture;
pub use hito_debrid_torbox::test_support::provider;
#[path = "support/requests.rs"]
mod requests;
#[path = "support/workflow.rs"]
mod workflow;

#[path = "support/state_inputs.rs"]
mod state_inputs;

use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::ProviderRecording;
use hito_plugin_torbox::{Config, CuratedWavRecording};
use reqwest::StatusCode;
use std::time::{Duration, Instant};

const SERVICE_TOKEN: &str = "owned-private-torbox-service-bearer";
const WRONG_TOKEN: &str = "owned-private-torbox-wrong-bearer";
pub const COOKIE: &str = "owned-private-torbox-caller-cookie";
const QUERY: &str = "owned-private-torbox";
const TITLE: &str = "owned-private-torbox-title";
const ARTIST: &str = "owned-private-torbox-artist";
const RECORDING: &str = "curated-recording-1";
const SOURCE: &str = "curated-wav-source-1";

#[tokio::test]
async fn authenticated_preparation_waits_then_revalidates_only_its_bound_wav_through_eof() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let wave = fixtures::wave();
        let media = media_fixture::MediaFixture::start(wave.clone()).await;
        let provider = provider::Fixture::start(vec![
            fixtures::observed(false, true, "album/owned.wav"),
            fixtures::observed(true, true, "album/owned.wav"),
            fixtures::acknowledgment(),
            fixtures::observed(true, true, "album/owned.wav"),
            fixtures::observed(true, true, "album/owned.wav"),
            fixtures::observed(true, true, "album/owned.wav"),
            fixtures::observed(true, true, "album/owned.wav"),
        ])
        .await;
        let binding = fixtures::binding();
        let original = serde_json::to_vec(&binding).unwrap();
        binding.validate().unwrap();
        let capture = logs::Capture::default();
        let inputs = state_inputs::Inputs::new();
        let plugin = state_inputs::Server::start(
            Config {
                state: inputs.next(),
                service_bearer: SERVICE_TOKEN.into(),
                client: TorBoxClient::with_test_tls_media(
                    fixtures::TOKEN.into(),
                    provider.ca_der(),
                    provider.address(),
                    media.ca_der(),
                    media.address(),
                )
                .unwrap(),
                media_origins: MediaAllowlist::new(vec![format!(
                    "https://{}",
                    fixtures::MEDIA_HOST
                )])
                .unwrap(),
                recordings: vec![CuratedWavRecording {
                    recording: recording(),
                    source_id: SOURCE.into(),
                    label: "Curated canonical PCM16 WAV".into(),
                    binding: binding.clone(),
                }],
            },
            capture.dispatch(),
            Some(inputs),
        )
        .await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(13))
            .build()
            .unwrap();
        let mut statuses = Vec::new();
        workflow::discover(&client, &plugin, &mut statuses).await;
        assert!(!workflow::sources(&client, &plugin, &mut statuses).await);
        let explicit_prepare = Instant::now();
        let receipt = workflow::prepare_and_poll(&client, &plugin, &mut statuses).await;
        assert!(workflow::sources(&client, &plugin, &mut statuses).await);
        let audio_path = format!("v1/audio?source_id={SOURCE}");
        let head = client
            .head(plugin.base.join(&audio_path).unwrap())
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", COOKIE)
            .send()
            .await
            .unwrap();
        statuses.push(head.status().as_u16());
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()["content-type"], "audio/wav");
        assert!(head.headers()["content-length"] == fixtures::WAV_BYTES.to_string());
        assert!(head.bytes().await.unwrap().is_empty());
        assert_eq!(
            media.requests(),
            0,
            "HEAD cannot consume the private candidate or open media"
        );
        let explicit_audio = Instant::now();
        let mut response = client
            .get(plugin.base.join(&audio_path).unwrap())
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", COOKIE)
            .send()
            .await
            .unwrap();
        statuses.push(response.status().as_u16());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "audio/wav");
        assert!(response.headers()["content-length"] == fixtures::WAV_BYTES.to_string());
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
        let mut received = Vec::new();
        while let Some(chunk) = response.chunk().await.unwrap() {
            assert!(chunk.len() <= (fixtures::WAV_BYTES as usize).saturating_sub(received.len()));
            received.extend_from_slice(&chunk);
        }
        assert!(received == wave);
        fixtures::assert_pcm(received);
        assert!(workflow::sources(&client, &plugin, &mut statuses).await);
        plugin.close().await.unwrap();
        let requests = provider.close().await;
        let media_requests = media.close().await;
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        requests::exact_authorities(&requests, &media_requests, explicit_prepare, explicit_audio);
        capture.assert_workflow(&statuses, &receipt.handle);
    })
    .await
    .expect("the owned HTTP/TLS TorBox preparation and exact WAV workflow exceeded its deadline");
}

fn recording() -> ProviderRecording {
    ProviderRecording {
        isrc: None,
        id: RECORDING.into(),
        title: TITLE.into(),
        artist: ARTIST.into(),
        duration_ms: Some(fixtures::FRAMES * 1000 / u64::from(fixtures::SAMPLE_RATE)),
        release: None,
        disc_number: None,
        track_number: None,
        revision: Some("curated-wav-v1".into()),
        attribution: None,
    }
}

fn private_json(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap();
    for private in [
        fixtures::TOKEN,
        SERVICE_TOKEN,
        WRONG_TOKEN,
        COOKIE,
        "0123456789abcdef0123456789abcdef01234567",
        "album/owned.wav",
        fixtures::MEDIA_TARGET,
        "api.torbox.app",
        "\"torrent_id\"",
        "\"file_id\"",
        "http://",
        "https://",
    ] {
        let escaped = serde_json::to_string(private).unwrap();
        assert!(!text.contains(private) && !text.contains(&escaped[1..escaped.len() - 1]));
    }
}
