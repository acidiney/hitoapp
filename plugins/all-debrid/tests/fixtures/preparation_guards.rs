use super::{
    ARTIST, COOKIE, DIRECTORY, FILE, HANDLE_KEY, HASH, MEDIA_HOST, MEDIA_TARGET, PROVIDER_TOKEN,
    RECORDING, SELECTED_LINK, SERVICE_TOKEN, SOURCE, TITLE, audio, http, logging, media,
    prepared_replies, private_json, recording, support,
};
use hito_debrid_all_debrid::{AllDebridClient, MagnetFileBinding, MediaAllowlist};
use hito_plugin_all_debrid::{Config, CuratedWavRecording, router};
use hito_plugin_sdk::{
    PreparationRequest, PreparationStatus, PreparationStatusRequest, PreparationStatusResponse,
    PreparationSubmission, PreparationSubmitResponse, SourcePage,
};
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn accepted_operation_cannot_be_retargeted_to_another_valid_catalog_entry() {
    tokio::time::timeout(Duration::from_secs(15), async {
    let wave = audio::wav();
    let media = media::MediaFixture::start(wave).await;
    let total = audio::WAV_BYTES as u64 * 2 + 400;
    let first = MagnetFileBinding {
        magnet_id: 7123,
        info_hash: HASH.into(),
        magnet_bytes: total,
        file_path: vec![DIRECTORY.into(), FILE.into()],
        file_bytes: audio::WAV_BYTES as u64,
    };
    let second = MagnetFileBinding {
        magnet_id: 7124,
        info_hash: "89abcdef0123456789abcdef0123456789abcdef".into(),
        magnet_bytes: total,
        file_path: vec!["owned-private-peer-directory".into(), "owned-private-peer.wav".into()],
        file_bytes: audio::WAV_BYTES as u64,
    };
    let mut replies = prepared_replies(&first, 2468, SELECTED_LINK);
    replies.extend(prepared_replies(&second, 2469,
        "https://alldebrid.com/f/owned-private-peer-selected"));
    let provider = support::Fixture::start(replies).await;
    let original = serde_json::to_vec(&(&first, &second)).unwrap();
    let capture = logging::Capture::default();
    let client = AllDebridClient::with_test_tls_media(
        PROVIDER_TOKEN.into(),provider.ca_der(),provider.address(),media.ca_der(),media.address(),
    ).unwrap();
    let mut peer_recording = recording();
    peer_recording.id = "curated-peer-recording".into();
    peer_recording.title = "owned-private-peer-title".into();
    let plugin = http::Server::start(router(Config {
        service_bearer: SERVICE_TOKEN.into(),receipt_key: HANDLE_KEY,client,
        media_origins: MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
        recordings: vec![
            CuratedWavRecording { recording:recording(),source_id:SOURCE.into(),
                label:"Curated canonical PCM16 WAV".into(),binding:first.clone() },
            CuratedWavRecording { recording:peer_recording,source_id:"curated-peer-source".into(),
                label:"Peer canonical PCM16 WAV".into(),binding:second.clone() },
        ],
    }).unwrap(),capture.dispatch()).await;
    let client = reqwest::Client::builder().no_proxy()
        .redirect(reqwest::redirect::Policy::none()).retry(reqwest::retry::never())
        .timeout(Duration::from_secs(4)).build().unwrap();
    let request = PreparationRequest {
        operation_id:Uuid::new_v4(),recording_id:RECORDING.into(),source_id:SOURCE.into(),
    };
    let (status,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/prepare",
        Some(SERVICE_TOKEN),Some(serde_json::to_value(&request).unwrap())).await;
    assert_eq!(status,StatusCode::OK);
    private_json(&bytes);
    let accepted:PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
    accepted.validate(&request).unwrap();
    let PreparationSubmission::Accepted { handle,status } = accepted.submission else {
        panic!("the first exact target must return its owned delayed receipt")
    };
    assert!(matches!(status,PreparationStatus::Preparing { poll_after_seconds:5,.. }));
    let retarget = PreparationRequest {
        operation_id:request.operation_id,
        recording_id:"curated-peer-recording".into(),source_id:"curated-peer-source".into(),
    };
    let (retarget_status,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/prepare",
        Some(SERVICE_TOKEN),Some(serde_json::to_value(&retarget).unwrap())).await;
    private_json(&bytes);
    let (status,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/prepare",
        Some(SERVICE_TOKEN),Some(serde_json::to_value(&request).unwrap())).await;
    assert_eq!(status,StatusCode::OK);
    private_json(&bytes);
    let replay:PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
    replay.validate(&request).unwrap();
    assert!(matches!(replay.submission,PreparationSubmission::Accepted { handle:replayed,.. }
        if replayed == handle));
    let (status,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources",Some(SERVICE_TOKEN),
        Some(json!({"recording_id":"curated-peer-recording"}))).await;
    assert_eq!(status,StatusCode::OK);
    private_json(&bytes);
    let peer:SourcePage = serde_json::from_slice(&bytes).unwrap();
    peer.validate().unwrap();
    assert_eq!(peer.items.len(),1);
    assert_eq!(peer.items[0].id,"curated-peer-source");
    assert!(!peer.items[0].ready);
    plugin.close().await.unwrap();
    let requests = provider.close().await;
    let media_requests = media.close().await;
    let unlocks = requests.iter().filter(|request| request.target == "/v4/link/unlock").count();
    assert_eq!(retarget_status,StatusCode::CONFLICT,
        "a reused operation must reject target replacement; actual provider unlock count={unlocks}");
    assert_eq!(unlocks,1);
    assert_eq!(requests.len(),3);
    assert!(media_requests.is_empty());
    assert!(serde_json::to_vec(&(&first,&second)).unwrap() == original);
    for (index,request) in requests.iter().enumerate() {
        let (target,body) = match index {
            0 => ("/v4.1/magnet/status","id=7123"),
            1 => ("/v4/magnet/files","id%5B%5D=7123"),
            2 => ("/v4/link/unlock","link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-private-service-selected"),
            _ => unreachable!(),
        };
        assert!(request.method == "POST" && request.target == target && request.body == body.as_bytes());
        assert!(request.server_name == "api.alldebrid.com");
        assert_eq!(request.header("authorization"),Some(format!("Bearer {PROVIDER_TOKEN}").as_str()));
        for private in ["cookie","referer","proxy-authorization","range","x-hito-plugin-protocol"] {
            assert!(request.header(private).is_none());
        }
    }
    capture.assert_retarget_rejected(&[
        PROVIDER_TOKEN,SERVICE_TOKEN,COOKIE,TITLE,ARTIST,HASH,DIRECTORY,FILE,SELECTED_LINK,&handle,
        &second.info_hash,"owned-private-peer-title","owned-private-peer-directory",
        "owned-private-peer.wav","https://alldebrid.com/f/owned-private-peer-selected",
    ]);

    }).await.expect("the bounded operation-retarget workflow exceeded its deadline");
}

#[tokio::test]
async fn delayed_candidate_never_publishes_ready_while_the_bound_magnet_is_processing() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let media = media::MediaFixture::start(audio::wav()).await;
        let generated = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
        let binding = MagnetFileBinding {
            magnet_id:7123,info_hash:HASH.into(),magnet_bytes:audio::WAV_BYTES as u64*2+400,
            file_path:vec![DIRECTORY.into(),FILE.into()],file_bytes:audio::WAV_BYTES as u64,
        };
        let original = serde_json::to_vec(&binding).unwrap();
        let mut replies = prepared_replies(&binding,2468,SELECTED_LINK);
        replies.push(serde_json::to_vec(&json!({
            "status":"success","data":{"status":2,"time_left":0,"link":generated}
        })).unwrap());
        let processing = serde_json::to_vec(&json!({"status":"success","data":{"magnets":[{
            "id":7123,"size":binding.magnet_bytes,"statusCode":1
        }]}})).unwrap();
        replies.extend((0..4).map(|_| processing.clone()));
        let provider = support::Fixture::start(replies).await;
        let capture = logging::Capture::default();
        let client = AllDebridClient::with_test_tls_media(PROVIDER_TOKEN.into(),
            provider.ca_der(),provider.address(),media.ca_der(),media.address()).unwrap();
        let plugin = http::Server::start(router(Config {
            service_bearer:SERVICE_TOKEN.into(),receipt_key:HANDLE_KEY,client,
            media_origins:MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            recordings:vec![CuratedWavRecording { recording:recording(),source_id:SOURCE.into(),
                label:"Curated canonical PCM16 WAV".into(),binding:binding.clone() }],
        }).unwrap(),capture.dispatch()).await;
        let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never()).timeout(Duration::from_secs(14)).build().unwrap();
        let request = PreparationRequest {
            operation_id:Uuid::new_v4(),recording_id:RECORDING.into(),source_id:SOURCE.into(),
        };
        let (code,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/prepare",
            Some(SERVICE_TOKEN),Some(serde_json::to_value(&request).unwrap())).await;
        assert_eq!(code,StatusCode::OK);
        private_json(&bytes);
        let accepted:PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
        accepted.validate(&request).unwrap();
        let PreparationSubmission::Accepted { handle,status } = accepted.submission else {
            panic!("the original delayed acknowledgment must retain its receipt")
        };
        assert!(matches!(status,PreparationStatus::Preparing { poll_after_seconds:5,.. }));
        let receipt = PreparationStatusRequest {
            operation_id:request.operation_id,recording_id:RECORDING.into(),source_id:SOURCE.into(),handle,
        };
        let (code,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/preparation/status",
            Some(SERVICE_TOKEN),Some(serde_json::to_value(&receipt).unwrap())).await;
        assert_eq!(code,StatusCode::OK);
        private_json(&bytes);
        let candidate_status:PreparationStatusResponse = serde_json::from_slice(&bytes).unwrap();
        candidate_status.validate(&receipt).unwrap();
        let (code,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources",Some(SERVICE_TOKEN),
            Some(json!({"recording_id":RECORDING}))).await;
        assert_eq!(code,StatusCode::OK);
        private_json(&bytes);
        let sources:SourcePage = serde_json::from_slice(&bytes).unwrap();
        sources.validate().unwrap();
        assert_eq!(sources.items.len(),1);
        assert_eq!(sources.items[0].id,SOURCE);
        assert!(!sources.items[0].ready);
        let (code,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/prepare",
            Some(SERVICE_TOKEN),Some(serde_json::to_value(&request).unwrap())).await;
        assert_eq!(code,StatusCode::OK);
        private_json(&bytes);
        let replay:PreparationSubmitResponse = serde_json::from_slice(&bytes).unwrap();
        replay.validate(&request).unwrap();
        let PreparationSubmission::Accepted { handle:replayed,status:replay_status } = replay.submission else {
            panic!("current readiness must not erase an acknowledged submission")
        };
        assert!(replayed == receipt.handle);
        let (code,bytes) = http::json(&client,&plugin,Method::POST,"v1/sources/preparation/status",
            Some(SERVICE_TOKEN),Some(serde_json::to_value(&receipt).unwrap())).await;
        assert_eq!(code,StatusCode::OK);
        private_json(&bytes);
        let later:PreparationStatusResponse = serde_json::from_slice(&bytes).unwrap();
        later.validate(&receipt).unwrap();
        plugin.close().await.unwrap();
        let requests = provider.close().await;
        let media_requests = media.close().await;
        let ready = usize::from(matches!(candidate_status.status,PreparationStatus::Ready {}))
            +usize::from(matches!(replay_status,PreparationStatus::Ready {}))
            +usize::from(matches!(later.status,PreparationStatus::Ready {}));
        let unlocks = requests.iter().filter(|request| request.target == "/v4/link/unlock").count();
        println!("Owned readiness checkpoint: ready_replies={ready} provider_requests={} unlocks={unlocks} media_requests={}",requests.len(),media_requests.len());
        for phase in [&candidate_status.status,&replay_status,&later.status] {
            assert!(matches!(phase,PreparationStatus::Preparing {
                progress_basis_points:None,eta_seconds:None,poll_after_seconds:5
            }),"the current bound magnet is processing; an opaque link cannot publish Ready");
        }
        assert_eq!(unlocks,1);
        assert_eq!(requests.len(),8);
        assert!(media_requests.is_empty());
        assert!(requests[3].received_at.duration_since(requests[2].received_at) >= Duration::from_secs(5));
        assert!(serde_json::to_vec(&binding).unwrap() == original);
        for (index,request) in requests.iter().enumerate() {
            let (target,body) = match index {
                0|4..=7 => ("/v4.1/magnet/status","id=7123"),
                1 => ("/v4/magnet/files","id%5B%5D=7123"),
                2 => ("/v4/link/unlock","link=https%3A%2F%2Falldebrid.com%2Ff%2Fowned-private-service-selected"),
                3 => ("/v4/link/delayed","id=2468"),
                _ => unreachable!(),
            };
            assert!(request.method == "POST" && request.target == target && request.body == body.as_bytes());
            assert!(request.server_name == "api.alldebrid.com");
            assert_eq!(request.header("authorization"),Some(format!("Bearer {PROVIDER_TOKEN}").as_str()));
            for private in ["cookie","referer","range","proxy-authorization","x-hito-plugin-protocol"] {
                assert!(request.header(private).is_none());
            }
        }
        capture.assert_processing_receipt(&[
            PROVIDER_TOKEN,SERVICE_TOKEN,COOKIE,TITLE,ARTIST,HASH,DIRECTORY,FILE,SELECTED_LINK,
            &generated,MEDIA_TARGET,&receipt.handle,
        ]);
    }).await.expect("the owned readiness workflow exceeded its total bound");
}
