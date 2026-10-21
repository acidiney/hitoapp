#![cfg(feature = "test-support")]

#[path = "support/admission_provider.rs"]
mod admission_provider;
#[path = "support/admission_workflow.rs"]
mod admission_workflow;
#[path = "../../hito-debrid-torbox/tests/audio/fixtures.rs"]
pub mod fixtures;
#[path = "../../hito-plugin-all-debrid/tests/fixtures/http.rs"]
pub mod http;
#[path = "../../hito-debrid-torbox/tests/audio/logs.rs"]
pub mod logs;
#[path = "../../hito-debrid-torbox/tests/audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "../../hito-debrid-torbox/tests/audio/scripted_media.rs"]
pub mod scripted_media;

use admission_provider::{Provider, Step};
use admission_workflow::{audio, head, prepare, ready, replay, selection};
#[path = "support/state_inputs.rs"]
mod state_inputs;

use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_torbox::Config;
use reqwest::StatusCode;
use std::time::Duration;

pub const COOKIE: &str = "owned-private-torbox-admission-cookie";
const SERVICE_TOKEN: &str = "owned-private-torbox-admission-bearer";

#[tokio::test]
async fn busy_audio_admission_keeps_the_ready_receipt_and_candidate_for_retry_without_reminting() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let (first_reply, first_closed) = scripted_media::Reply::held(fixtures::wave());
        let (second_reply, second_closed) = scripted_media::Reply::held(fixtures::wave());
        let media = scripted_media::ScriptedMedia::start(vec![
            first_reply,
            second_reply,
            scripted_media::Reply::full(fixtures::wave()),
        ])
        .await;
        let provider = Provider::start(vec![
            Step::Observe(0),
            Step::Mint(0),
            Step::Observe(1),
            Step::Mint(1),
            Step::Observe(2),
            Step::Mint(2),
            Step::Observe(0),
            Step::Observe(1),
            Step::Observe(2),
            Step::Observe(2),
            Step::Observe(2),
        ])
        .await;
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
                recordings: (0..3).map(selection).collect(),
            },
            capture.dispatch(),
            Some(inputs),
        )
        .await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap();
        let _first_receipt = prepare(&client, &plugin, 0).await;
        let _second_receipt = prepare(&client, &plugin, 1).await;
        let (request, receipt) = prepare(&client, &plugin, 2).await;
        assert_eq!(provider.requests(), (6, 3));
        assert_eq!(media.requests(), 0);

        let mut first = audio(&client, &plugin, 0).await;
        let mut second = audio(&client, &plugin, 1).await;
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        assert!(first.chunk().await.unwrap().unwrap().as_ref() == &fixtures::wave()[..44]);
        assert!(second.chunk().await.unwrap().unwrap().as_ref() == &fixtures::wave()[..44]);
        assert_eq!(provider.requests(), (8, 3));
        assert_eq!(media.requests(), 2);

        let busy = audio(&client, &plugin, 2).await;
        assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
        admission_workflow::private_json(&busy.bytes().await.unwrap());
        assert_eq!(provider.requests(), (8, 3));
        assert_eq!(media.requests(), 2);

        replay(&client, &plugin, &request, &receipt).await;
        assert_eq!(provider.requests(), (8, 3));
        assert_eq!(media.requests(), 2);
        assert!(ready(&client, &plugin, 2).await);
        head(&client, &plugin, 2).await;
        assert_eq!(provider.requests(), (10, 3));
        assert_eq!(media.requests(), 2);

        drop(first);
        tokio::time::timeout(Duration::from_secs(1), first_closed)
            .await
            .unwrap()
            .unwrap();
        let mut recovered = audio(&client, &plugin, 2).await;
        assert_eq!(recovered.status(), StatusCode::OK);
        assert_eq!(recovered.headers()["content-type"], "audio/wav");
        assert!(recovered.headers()["content-length"] == fixtures::WAV_BYTES.to_string());
        let mut received = Vec::new();
        while let Some(chunk) = recovered.chunk().await.unwrap() {
            assert!(chunk.len() <= fixtures::WAV_BYTES as usize - received.len());
            received.extend_from_slice(&chunk);
        }
        assert!(received == fixtures::wave());
        fixtures::assert_pcm(received);
        assert_eq!(provider.requests(), (11, 3));
        assert_eq!(media.requests(), 3);
        drop(second);
        tokio::time::timeout(Duration::from_secs(1), second_closed)
            .await
            .unwrap()
            .unwrap();
        plugin.close().await.unwrap();
        assert_eq!(provider.close().await, (11, 3));
        assert_eq!(media.close().await, 3);
        capture.assert_operations(
            &[
                ("candidate_authority", "complete"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "busy"),
                ("torrent_observation", "provider_completed"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "cancelled"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "cancelled"),
            ],
            &[
                fixtures::TOKEN,
                SERVICE_TOKEN,
                COOKIE,
                "owned-private-admission-title",
                "owned-private-admission-artist",
                "album/owned.wav",
                &receipt.handle,
            ],
        );
    })
    .await
    .expect("the owned HTTP/TLS TorBox admission recovery exceeded its deadline");
}
