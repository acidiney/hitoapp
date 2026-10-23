use super::{
    ARTIST, COOKIE, DIRECTORY, FILE, HANDLE_KEY, HASH, MEDIA_HOST, MEDIA_TARGET, PROVIDER_TOKEN,
    QUERY, RECORDING, SELECTED_LINK, SERVICE_TOKEN, SOURCE, TITLE, WRONG_TOKEN, audio, http,
    logging, media, private_json, recording, sources, support,
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetFileBinding, MediaAllowlist};
use hito_plugin_all_debrid::{Config, CuratedWavRecording, router};
use hito_plugin_sdk::{
    Capability, Manifest, PreparationRequest, PreparationStatus, PreparationStatusRequest,
    PreparationStatusResponse, PreparationSubmission, PreparationSubmitResponse, ProviderRecording,
    SearchPage, SourcePersistence,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[tokio::test]
async fn authenticated_preparation_receipt_reaches_only_its_current_bound_wav() {
    tokio::time::timeout(Duration::from_secs(25), async {
        let wave = audio::wav();
        let media = media::MediaFixture::start(wave.clone()).await;
        let generated = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
        let total = audio::WAV_BYTES as u64 * 2 + 400;
        let status = json!({"status":"success","data":{"magnets":[{
            "id":7123,"size":total,"statusCode":4
        }]}});
        let files = json!({"status":"success","data":{"magnets":[{
            "id":"7123","files":[
                {"n":"other","e":[{"n":FILE,"s":audio::WAV_BYTES,
                    "l":"https://alldebrid.com/f/owned-private-service-decoy"}]},
                {"n":DIRECTORY,"e":[{"n":FILE,"s":audio::WAV_BYTES,"l":SELECTED_LINK}]},
                {"n":"readme.txt","s":400,"l":"https://alldebrid.com/f/owned-private-note"}
            ]
        }]}});
        let provider = support::Fixture::start(
            [
                status.clone(),
                files.clone(),
                json!({"status":"success","data":{"delayed":2468,"filesize":audio::WAV_BYTES}}),
                json!({"status":"success","data":{"status":2,"time_left":0,"link":generated}}),
                status.clone(),
                files.clone(),
                status.clone(),
                files.clone(),
                status,
                files,
            ]
            .into_iter()
            .map(|value| serde_json::to_vec(&value).unwrap())
            .collect(),
        )
        .await;
        let binding = MagnetFileBinding {
            magnet_id: 7123,
            info_hash: HASH.into(),
            magnet_bytes: total,
            file_path: vec![DIRECTORY.into(), FILE.into()],
            file_bytes: audio::WAV_BYTES as u64,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let provider_client = AllDebridClient::with_test_tls_media(
            PROVIDER_TOKEN.into(),
            provider.ca_der(),
            provider.address(),
            media.ca_der(),
            media.address(),
        )
        .unwrap();
        let configured = CuratedWavRecording {
            recording: recording(),
            source_id: SOURCE.into(),
            label: "Curated canonical PCM16 WAV".into(),
            binding: binding.clone(),
        };
        let capture = logging::Capture::default();
        let plugin = http::Server::start(
            router(Config {
                service_bearer: SERVICE_TOKEN.into(),
                receipt_key: HANDLE_KEY,
                client: provider_client,
                media_origins: MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
                recordings: vec![configured],
            })
            .unwrap(),
            capture.dispatch(),
        )
        .await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(13))
            .build()
            .unwrap();
        let request = PreparationRequest {
            operation_id: Uuid::new_v4(),
            recording_id: RECORDING.into(),
            source_id: SOURCE.into(),
        };
        let operation = request.operation_id;
        let mut statuses = Vec::new();
        let (code, body) =
            http::json(&client, &plugin, Method::GET, "v1/manifest", None, None).await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::OK);
        private_json(&body);
        let manifest: Manifest = serde_json::from_slice(&body).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.id, "org.hito.all-debrid");
        assert!(
            manifest.capabilities
                == [
                    Capability::Search,
                    Capability::Sources,
                    Capability::SourcePreparation
                ]
                .into()
        );
        assert_eq!(
            manifest.content_policy.unwrap().source_persistence,
            SourcePersistence::PlaybackOnly
        );
        for token in [None, Some(WRONG_TOKEN)] {
            for (method, path, body) in [
                (
                    Method::POST,
                    "v1/search",
                    Some(json!({"query":QUERY,"cursor":null,"limit":10})),
                ),
                (
                    Method::POST,
                    "v1/recordings/detail",
                    Some(json!({"recording_id":RECORDING})),
                ),
                (
                    Method::POST,
                    "v1/sources",
                    Some(json!({"recording_id":RECORDING})),
                ),
                (
                    Method::POST,
                    "v1/sources/prepare",
                    Some(serde_json::to_value(&request).unwrap()),
                ),
                (
                    Method::POST,
                    "v1/sources/preparation/status",
                    Some(json!({
                        "operation_id":operation,"recording_id":RECORDING,"source_id":SOURCE,
                        "handle":"unowned-handle"
                    })),
                ),
                (Method::GET, "v1/audio?source_id=curated-wav-source-1", None),
            ] {
                let (code, body) = http::json(&client, &plugin, method, path, token, body).await;
                statuses.push(code.as_u16());
                assert_eq!(code, StatusCode::UNAUTHORIZED);
                private_json(&body);
            }
        }
        let (code, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/search",
            Some(SERVICE_TOKEN),
            Some(json!({"query":QUERY,"cursor":null,"limit":10})),
        )
        .await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::OK);
        private_json(&body);
        let search: SearchPage = serde_json::from_slice(&body).unwrap();
        search.validate(10).unwrap();
        assert!(search.items == vec![recording()] && search.next_cursor.is_none());
        let (code, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/recordings/detail",
            Some(SERVICE_TOKEN),
            Some(json!({"recording_id":RECORDING})),
        )
        .await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::OK);
        private_json(&body);
        let detail: ProviderRecording = serde_json::from_slice(&body).unwrap();
        detail.validate().unwrap();
        assert!(detail == recording());
        let before = sources(&client, &plugin, &mut statuses).await;
        assert!(
            !before.items[0].ready,
            "provider metadata alone cannot publish a ready source"
        );
        let explicit_prepare = Instant::now();
        let (code, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/sources/prepare",
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(&request).unwrap()),
        )
        .await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::OK);
        private_json(&body);
        let submitted: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
        submitted.validate(&request).unwrap();
        let PreparationSubmission::Accepted { handle, status } = submitted.submission else {
            panic!("an exact delayed acknowledgment must produce an owned preparation receipt")
        };
        assert!(matches!(
            status,
            PreparationStatus::Preparing {
                progress_basis_points: None,
                poll_after_seconds: 5,
                ..
            }
        ));
        let stored = serde_json::to_vec(&PreparationStatusRequest {
            operation_id: operation,
            recording_id: RECORDING.into(),
            source_id: SOURCE.into(),
            handle,
        })
        .unwrap();
        private_json(&stored);
        let receipt: PreparationStatusRequest = serde_json::from_slice(&stored).unwrap();
        receipt.validate().unwrap();
        assert!(receipt.handle.len() <= hito_plugin_sdk::MAX_PREPARATION_HANDLE_BYTES);
        let (code, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/sources/prepare",
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(&request).unwrap()),
        )
        .await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::OK);
        private_json(&body);
        let replay: PreparationSubmitResponse = serde_json::from_slice(&body).unwrap();
        replay.validate(&request).unwrap();
        assert!(
            matches!(replay.submission,PreparationSubmission::Accepted {handle,..}
            if handle == receipt.handle)
        );
        let mut forged: Value = serde_json::from_slice(&stored).unwrap();
        forged["operation_id"] = json!(Uuid::new_v4());
        let (code, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/sources/preparation/status",
            Some(SERVICE_TOKEN),
            Some(forged),
        )
        .await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::NOT_FOUND);
        private_json(&body);
        let (code, body) = http::json(
            &client,
            &plugin,
            Method::POST,
            "v1/sources/preparation/status",
            Some(SERVICE_TOKEN),
            Some(serde_json::to_value(&receipt).unwrap()),
        )
        .await;
        statuses.push(code.as_u16());
        assert_eq!(code, StatusCode::OK);
        private_json(&body);
        let completed: PreparationStatusResponse = serde_json::from_slice(&body).unwrap();
        completed.validate(&receipt).unwrap();
        assert!(matches!(completed.status, PreparationStatus::Ready {}));
        assert!(sources(&client, &plugin, &mut statuses).await.items[0].ready);
        let explicit_audio = Instant::now();
        let mut response = client
            .get(
                plugin
                    .base
                    .join("v1/audio?source_id=curated-wav-source-1")
                    .unwrap(),
            )
            .bearer_auth(SERVICE_TOKEN)
            .header("x-hito-plugin-protocol", "1")
            .header("cookie", COOKIE)
            .send()
            .await
            .unwrap();
        statuses.push(response.status().as_u16());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "audio/wav");
        assert_eq!(
            response.headers()["content-length"],
            audio::WAV_BYTES.to_string()
        );
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        for forbidden in [
            "location",
            "set-cookie",
            "content-disposition",
            "etag",
            "content-range",
            "accept-ranges",
        ] {
            assert!(response.headers().get(forbidden).is_none());
        }
        let mut received = Vec::new();
        while let Some(chunk) = response.chunk().await.unwrap() {
            assert!(chunk.len() <= audio::WAV_BYTES.saturating_sub(received.len()));
            received.extend_from_slice(&chunk);
        }
        assert!(received == wave);
        audio::assert_decoded(received);
        plugin.close().await.unwrap();
        let requests = provider.close().await;
        let media_requests = media.close().await;
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        assert_eq!(requests.len(), 10);
        assert_eq!(media_requests.len(), 1);
        assert!(requests[0].received_at >= explicit_prepare);
        assert!(
            requests[3]
                .received_at
                .duration_since(requests[2].received_at)
                >= Duration::from_secs(5)
        );
        assert!(media_requests[0].received_at >= explicit_audio);
        assert!(media_requests[0].received_at >= requests[9].received_at);
        for (index, request) in requests.iter().enumerate() {
            let (target, body) = match index {
                0 | 4 | 6 | 8 => ("/v4.1/magnet/status", "id=7123"),
                1 | 5 | 7 | 9 => ("/v4/magnet/files", "id%5B%5D=7123"),
                2 => (
                    "/v4/link/unlock",
                    "link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-private-service-selected",
                ),
                3 => ("/v4/link/delayed", "id=2468"),
                _ => unreachable!(),
            };
            assert!(
                request.method == "POST"
                    && request.target == target
                    && request.body == body.as_bytes()
            );
            assert!(request.server_name == "api.alldebrid.com");
            assert_eq!(request.header("host"), Some("api.alldebrid.com"));
            assert_eq!(
                request.header("authorization"),
                Some(format!("Bearer {PROVIDER_TOKEN}").as_str())
            );
            assert_eq!(
                request.header("content-type"),
                Some("application/x-www-form-urlencoded")
            );
            assert_eq!(request.header("accept-encoding"), Some("identity"));
            for name in [
                "cookie",
                "referer",
                "range",
                "proxy-authorization",
                "x-hito-plugin-protocol",
            ] {
                assert!(request.header(name).is_none());
            }
        }
        let media_request = &media_requests[0];
        assert!(
            media_request.method == "GET"
                && media_request.target == MEDIA_TARGET
                && media_request.server_name == MEDIA_HOST
                && media_request.body.is_empty()
        );
        assert_eq!(media_request.header("host"), Some(MEDIA_HOST));
        assert_eq!(media_request.header("accept-encoding"), Some("identity"));
        for name in [
            "authorization",
            "cookie",
            "referer",
            "range",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(media_request.header(name).is_none());
        }
        capture.assert_workflow(
            &statuses,
            &[
                PROVIDER_TOKEN,
                SERVICE_TOKEN,
                WRONG_TOKEN,
                COOKIE,
                QUERY,
                TITLE,
                ARTIST,
                HASH,
                DIRECTORY,
                FILE,
                SELECTED_LINK,
                &generated,
                MEDIA_TARGET,
                &receipt.handle,
            ],
            audio::WAV_BYTES as u64,
        );
    })
    .await
    .expect("the complete owned service workflow exceeded its total bound");
}
