#![cfg(feature = "test-support")]

#[path = "support/concurrent_provider.rs"]
pub mod concurrent_provider;
pub use hito_debrid_torbox::test_support::fixtures;
pub use hito_debrid_torbox::test_support::logs;
pub use hito_debrid_torbox::test_support::owned_tls;
pub use hito_debrid_torbox::test_support::provider;
pub use hito_testkit::plugin_server as http;
#[path = "support/retirement_race.rs"]
mod retirement_race;
pub use hito_debrid_torbox::test_support::scripted_media;
#[path = "support/reusable_workflow.rs"]
pub mod workflow;

use concurrent_provider::{ConcurrentProvider, Exchange, completed};
#[path = "support/state_inputs.rs"]
mod state_inputs;

use hito_debrid_torbox::{MediaAllowlist, TorBoxClient};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, ProviderRecording, SourcePage,
};
use hito_plugin_torbox::{Config, CuratedWavRecording};
use provider::reply::Reply;
use reqwest::{Method, StatusCode};
use std::time::Duration;
pub use workflow::COOKIE;
use workflow::SERVICE_TOKEN;

const RECORDING: &str = "curated-control-recording";
const SOURCE: &str = "curated-control-source";

#[tokio::test]
async fn held_control_observations_do_not_consume_the_ready_audio_candidate() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let (first, first_received, first_release) =
            Reply::paused(fixtures::observed(true, true, "album/owned.wav"));
        let (second, second_received, second_release) =
            Reply::paused(fixtures::observed(true, true, "album/owned.wav"));
        let provider = ConcurrentProvider::start(vec![
            completed(),
            Exchange::Mint(Reply::json(fixtures::acknowledgment())),
            Exchange::Observe(first),
            Exchange::Observe(second),
            completed(),
        ])
        .await;
        let media = scripted_media::ScriptedMedia::start(vec![scripted_media::Reply::full(
            fixtures::wave(),
        )])
        .await;
        let capture = logs::Capture::default();
        let (service, client) = start(&provider, &media, capture.dispatch()).await;
        let (_, receipt) = prepare(&client, &service).await;
        let first = source_request(&client, &service);
        let first = tokio::spawn(async move { first.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), first_received)
            .await
            .unwrap()
            .unwrap();
        let second = source_request(&client, &service);
        let second = tokio::spawn(async move { second.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), second_received)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(provider.requests(), (4, 1));
        let failed = audio_request(&client, &service).send().await.unwrap();
        assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(failed.headers()["cache-control"], "private, no-store");
        workflow::private_json(&failed.bytes().await.unwrap());
        assert_eq!(provider.requests(), (4, 1));
        assert_eq!(media.requests(), 0);
        first_release.send(()).unwrap();
        second_release.send(()).unwrap();
        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(first.headers()["cache-control"], "private, no-store");
        assert_eq!(second.headers()["cache-control"], "private, no-store");
        let first = first.bytes().await.unwrap();
        let second = second.bytes().await.unwrap();
        workflow::private_json(&first);
        workflow::private_json(&second);
        let first: SourcePage = serde_json::from_slice(&first).unwrap();
        let second: SourcePage = serde_json::from_slice(&second).unwrap();
        first.validate().unwrap();
        second.validate().unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(second.items.len(), 1);
        assert!(first.items[0].id == SOURCE && second.items[0].id == SOURCE);
        let status: PreparationStatusResponse =
            post(&client, &service, "v1/sources/preparation/status", &receipt).await;
        status.validate(&receipt).unwrap();
        assert!(
            matches!(status.status, PreparationStatus::Ready {}),
            "control admission rejection must retain the actual ready candidate"
        );
        assert!(first.items[0].ready && second.items[0].ready);
        workflow::complete(audio_request(&client, &service).send().await.unwrap()).await;
        assert_eq!(provider.requests(), (5, 1));
        assert_eq!(media.requests(), 1);
        service.close().await.unwrap();
        assert_eq!(provider.close().await, (5, 1));
        assert_eq!(media.close().await, 1);
        capture.assert_operations(
            &[
                ("candidate_authority", "complete"),
                ("torrent_observation", "provider_completed"),
                ("bound_file_preparation", "candidate"),
                ("candidate_sealing", "sealed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "busy"),
                ("bound_audio_opening", "busy"),
                ("torrent_observation", "provider_completed"),
                ("torrent_observation", "provider_completed"),
                ("candidate_restoration", "restored"),
                ("torrent_observation", "provider_completed"),
                ("bound_audio_opening", "complete"),
                ("bound_audio_opening", "complete"),
            ],
            &[
                fixtures::TOKEN,
                SERVICE_TOKEN,
                COOKIE,
                "album/owned.wav",
                &receipt.handle,
            ],
        );
    })
    .await
    .expect("the owned TorBox control-admission workflow exceeded its deadline");
}

#[tokio::test]
async fn a_provider_rate_limit_keeps_the_ready_candidate_for_an_explicit_retry() {
    tokio::time::timeout(Duration::from_secs(7), async {
        let mut rate_limited = Reply::json(fixtures::observed(true, true, "album/owned.wav"));
        rate_limited.status = 429;
        let provider = ConcurrentProvider::start(vec![completed(), Exchange::Mint(Reply::json(fixtures::acknowledgment())), Exchange::Observe(rate_limited), completed()]).await;
        let media = scripted_media::ScriptedMedia::start(vec![scripted_media::Reply::full(fixtures::wave())]).await;
        let capture = logs::Capture::default();
        let (service, client) = start(&provider, &media, capture.dispatch()).await;
        let (request, receipt) = prepare(&client, &service).await;
        let failed = audio_request(&client, &service).send().await.unwrap();
        assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
        workflow::private_json(&failed.bytes().await.unwrap());
        assert_eq!(provider.requests(), (3, 1));
        assert_eq!(media.requests(), 0);
        let status: PreparationStatusResponse = post(&client, &service, "v1/sources/preparation/status", &receipt).await;
        status.validate(&receipt).unwrap();
        assert!(matches!(status.status, PreparationStatus::Ready {}));
        let replay: PreparationSubmitResponse = post(&client, &service, "v1/sources/prepare", &request).await;
        replay.validate(&request).unwrap();
        assert!(matches!(replay.submission, PreparationSubmission::Accepted { handle, status: PreparationStatus::Ready {} } if handle == receipt.handle));
        workflow::complete(audio_request(&client, &service).send().await.unwrap()).await;
        assert_eq!(provider.requests(), (4, 1));
        assert_eq!(media.requests(), 1);
        service.close().await.unwrap();
        assert_eq!(provider.close().await, (4, 1));
        assert_eq!(media.close().await, 1);
        capture.assert_operations(&[
            ("candidate_authority", "complete"),
            ("torrent_observation", "provider_completed"), ("bound_file_preparation", "candidate"),
            ("candidate_sealing", "sealed"),
            ("candidate_restoration", "restored"),
            ("torrent_observation", "http_rejected"), ("bound_audio_opening", "http_rejected"),
            ("candidate_restoration", "restored"),
            ("torrent_observation", "provider_completed"), ("bound_audio_opening", "complete"), ("bound_audio_opening", "complete"),
        ], &[fixtures::TOKEN, SERVICE_TOKEN, COOKIE, "album/owned.wav", &receipt.handle]);
    }).await.expect("the owned TorBox provider rate-limit control exceeded its deadline");
}

async fn start(
    provider: &ConcurrentProvider,
    media: &scripted_media::ScriptedMedia,
    dispatch: tracing::Dispatch,
) -> (state_inputs::Server, reqwest::Client) {
    let inputs = state_inputs::Inputs::new();
    let service = state_inputs::Server::start(
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
            media_origins: MediaAllowlist::new(vec![format!("https://{}", fixtures::MEDIA_HOST)])
                .unwrap(),
            recordings: vec![CuratedWavRecording {
                recording: ProviderRecording {
                    isrc: None,
                    id: RECORDING.into(),
                    title: "owned-private-control-title".into(),
                    artist: "owned-private-control-artist".into(),
                    duration_ms: Some(fixtures::FRAMES * 1000 / u64::from(fixtures::SAMPLE_RATE)),
                    release: None,
                    disc_number: None,
                    track_number: None,
                    revision: None,
                    attribution: None,
                },
                source_id: SOURCE.into(),
                label: "Curated canonical PCM16 WAV".into(),
                binding: fixtures::binding(),
            }],
        },
        dispatch,
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
    (service, client)
}

fn source_request(client: &reqwest::Client, service: &http::Server) -> reqwest::RequestBuilder {
    client
        .post(service.base.join("v1/sources").unwrap())
        .bearer_auth(SERVICE_TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", COOKIE)
        .json(&serde_json::json!({"recording_id":RECORDING}))
}
fn audio_request(client: &reqwest::Client, service: &http::Server) -> reqwest::RequestBuilder {
    client
        .get(
            service
                .base
                .join(&format!("v1/audio?source_id={SOURCE}"))
                .unwrap(),
        )
        .bearer_auth(SERVICE_TOKEN)
        .header("x-hito-plugin-protocol", "1")
        .header("cookie", COOKIE)
}
async fn post<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    service: &http::Server,
    path: &str,
    request: &impl serde::Serialize,
) -> T {
    let (status, bytes) = http::json(
        client,
        service,
        Method::POST,
        path,
        COOKIE,
        Some(SERVICE_TOKEN),
        Some(serde_json::to_value(request).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    workflow::private_json(&bytes);
    serde_json::from_slice(&bytes).unwrap()
}

async fn prepare(
    client: &reqwest::Client,
    service: &http::Server,
) -> (PreparationRequest, PreparationStatusRequest) {
    let request = PreparationRequest {
        operation_id: uuid::Uuid::new_v4(),
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
    };
    let submitted: PreparationSubmitResponse =
        post(client, service, "v1/sources/prepare", &request).await;
    submitted.validate(&request).unwrap();
    let PreparationSubmission::Accepted {
        handle,
        status: PreparationStatus::Ready {},
    } = submitted.submission
    else {
        panic!("the owned preparation must become ready")
    };
    let receipt = PreparationStatusRequest {
        operation_id: request.operation_id,
        recording_id: RECORDING.into(),
        source_id: SOURCE.into(),
        handle,
    };
    (request, receipt)
}
