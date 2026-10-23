use super::{
    ARTIST, COOKIE, DIRECTORY, FILE, HANDLE_KEY, HASH, MEDIA_HOST, PROVIDER_TOKEN, RECORDING,
    SELECTED_LINK, SERVICE_TOKEN, SOURCE, TITLE, audio, http, logging, media, prepared_replies,
    private_json, recording, support,
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetFileBinding, MediaAllowlist};
use hito_plugin_all_debrid::{Config, CuratedWavRecording, router};
use hito_plugin_sdk::{
    PreparationReason, PreparationRequest, PreparationStatus, PreparationSubmission,
    PreparationSubmitResponse,
};
use reqwest::{Method, StatusCode};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[tokio::test]
async fn pre_unlock_rate_limit_can_retry_the_same_operation_after_its_guard() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let media = media::MediaFixture::start(audio::wav()).await;
        let binding = MagnetFileBinding {
            magnet_id: 7123, info_hash: HASH.into(), magnet_bytes: audio::WAV_BYTES as u64 * 2 + 400,
            file_path: vec![DIRECTORY.into(), FILE.into()], file_bytes: audio::WAV_BYTES as u64,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let mut replies = vec![support::Reply::RateLimited];
        replies.extend(prepared_replies(&binding, 2468, SELECTED_LINK).into_iter().map(support::Reply::from));
        let provider = support::Fixture::start(replies).await;
        let capture = logging::Capture::default();
        let client = AllDebridClient::with_test_tls_media(PROVIDER_TOKEN.into(), provider.ca_der(),
            provider.address(), media.ca_der(), media.address()).unwrap();
        let plugin = http::Server::start(router(Config {
            service_bearer: SERVICE_TOKEN.into(), receipt_key: HANDLE_KEY, client,
            media_origins: MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            recordings: vec![CuratedWavRecording { recording: recording(), source_id: SOURCE.into(),
                label: "Curated canonical PCM16 WAV".into(), binding: binding.clone() }],
        }).unwrap(), capture.dispatch()).await;
        let client = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never())
            .timeout(Duration::from_secs(4)).build().unwrap();
        let request = PreparationRequest {
            operation_id: Uuid::new_v4(), recording_id: RECORDING.into(), source_id: SOURCE.into(),
        };
        let first = prepare(&client, &plugin, &request).await;
        let first_finished = Instant::now();
        let PreparationSubmission::NotSubmitted {
            reason: PreparationReason::RateLimited, retry_after_seconds: Some(delay),
        } = first.submission else {
            panic!("a pre-unlock429 must remain definitely unsubmitted with a retry delay")
        };
        assert_eq!(delay, 5);
        let immediate = prepare(&client, &plugin, &request).await;
        tokio::time::sleep(Duration::from_secs(u64::from(delay))).await;
        let retry_started = Instant::now();
        let recovered = prepare(&client, &plugin, &request).await;
        let replay = prepare(&client, &plugin, &request).await;
        plugin.close().await.unwrap();
        let requests = provider.close().await;
        let media_requests = media.close().await;
        let unlocks = requests.iter().filter(|request| request.target == "/v4/link/unlock").count();
        let accepted = matches!(&recovered.submission, PreparationSubmission::Accepted { .. });
        println!("Owned pre-unlock retry checkpoint: accepted={} provider_requests={} unlocks={unlocks} media_requests={}",
            usize::from(accepted), requests.len(), media_requests.len());
        let PreparationSubmission::Accepted { handle, status } = recovered.submission else {
            panic!("a definitely unsubmitted operation must retry its current provider after recovery with the same operation_id")
        };
        assert!(matches!(status, PreparationStatus::Preparing {
            progress_basis_points: None, eta_seconds: None, poll_after_seconds: 5
        }));
        assert!(matches!(immediate.submission, PreparationSubmission::NotSubmitted {
            reason: PreparationReason::RateLimited, retry_after_seconds: Some(5)
        }));
        assert!(matches!(replay.submission, PreparationSubmission::Accepted { handle: replayed,
            status: PreparationStatus::Preparing { poll_after_seconds: 5, .. } } if replayed == handle));
        assert_eq!(requests.len(), 4);
        assert_eq!(unlocks, 1);
        assert!(media_requests.is_empty());
        assert!(requests[0].received_at <= first_finished);
        assert!(requests[1].received_at >= retry_started);
        assert!(requests[1].received_at.duration_since(requests[0].received_at) >= Duration::from_secs(5));
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        for (index, request) in requests.iter().enumerate() {
            let (target, body) = match index {
                0 | 1 => ("/v4.1/magnet/status", "id=7123"),
                2 => ("/v4/magnet/files", "id%5B%5D=7123"),
                3 => ("/v4/link/unlock", "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-private-service-selected"),
                _ => unreachable!(),
            };
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
        capture.assert_pre_unlock_retry(&[
            PROVIDER_TOKEN, SERVICE_TOKEN, COOKIE, TITLE, ARTIST, HASH, DIRECTORY, FILE, SELECTED_LINK,
            &handle, "owned-private-retarget-decoy", "owned-private-retarget-note",
        ]);
    }).await.expect("the bounded pre-unlock retry workflow exceeded its deadline");
}

async fn prepare(
    client: &reqwest::Client,
    plugin: &http::Server,
    request: &PreparationRequest,
) -> PreparationSubmitResponse {
    let (code, bytes) = http::json(
        client,
        plugin,
        Method::POST,
        "v1/sources/prepare",
        Some(SERVICE_TOKEN),
        Some(serde_json::to_value(request).unwrap()),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    private_json(&bytes);
    let response: PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
    response.validate(request).unwrap();
    response
}
