use hito_core::test_support::podcast_audio_service as fixture;
#[path = "../fixtures/artwork_http.rs"]
mod http;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{CoreConfig, HostDeviceId, PodcastNetworkConfig, PodcastSubscriptionReceipt};
use http::{call, get};
use serde_json::json;
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

#[tokio::test]
async fn the_host_refreshes_private_podcasts_and_relays_audio_without_a_download() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = support::write_wav(&directory.path().join("owned-podcast.wav"));
    let mut fixture = PodcastAudioServer::start(audio.clone()).await;
    let mut core = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("podcast-runtime").unwrap(),
    );
    core.podcast_network = PodcastNetworkConfig::new(vec![fixture.origin.clone()]).unwrap();
    let services = hito_core::open(core).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = hito_server::Config {
        origin: format!("http://{address}"),
        ..Default::default()
    };
    let (stop, stopped) = oneshot::channel();
    let mut serving = tokio::spawn(hito_server::serve(
        listener,
        hito_server::router_with_config(services.clone(), config),
        services,
        async {
            let _ = stopped.await;
        },
    ));
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(address, &fixture, &audio)).catch_unwind(),
    )
    .await;
    let _ = stop.send(());
    let host_joined = match tokio::time::timeout(Duration::from_secs(7), &mut serving).await {
        Ok(result) => matches!(result, Ok(Ok(()))),
        Err(_) => {
            serving.abort();
            let _ = serving.await;
            false
        }
    };
    let fixture_joined = fixture.finish().await;
    assert!(
        host_joined,
        "shutdown must join HTTP and its background workers"
    );
    assert!(fixture_joined, "the owned RSS and audio service must join");
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public podcast runtime exercise must finish within its deadline"),
    }
}

async fn exercise(address: SocketAddr, fixture: &PodcastAudioServer, audio: &[u8]) {
    let setup = call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({"username": "owner", "password": "owned podcast runtime password"})),
        None,
    )
    .await;
    assert_eq!(setup.status, StatusCode::CREATED);
    let session = setup.json();
    let token = session["token"].as_str().unwrap();
    let request_id = Uuid::new_v4();
    let subscribed = call(
        address,
        "POST",
        "/api/v1/podcasts",
        Some(json!({"request_id": request_id, "feed_url": fixture.feed_url})),
        Some(token),
    )
    .await;
    assert_eq!(subscribed.status, StatusCode::ACCEPTED);
    let receipt = subscribed.json();
    let receipt = PodcastSubscriptionReceipt {
        request_id: serde_json::from_value(receipt["request_id"].clone()).unwrap(),
        subscription_id: serde_json::from_value(receipt["subscription_id"].clone()).unwrap(),
        job_id: serde_json::from_value(receipt["job_id"].clone()).unwrap(),
    };
    assert_eq!(receipt.request_id, request_id);
    assert!(!receipt.subscription_id.is_nil());
    assert!(!receipt.job_id.is_nil());
    let episodes_path = format!("/api/v1/podcasts/{}/episodes", receipt.subscription_id);
    let episodes = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let page = get(address, token, &episodes_path).await;
            if page["total"] == 1 {
                break page;
            }
            assert_eq!(page["total"], 0);
            assert_eq!(page["items"], json!([]));
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("the host workers must fetch and publish the initial feed");
    let subscriptions = get(address, token, "/api/v1/podcasts").await;
    let subscriptions = subscriptions.as_array().unwrap();
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(
        subscriptions[0]["subscription_id"],
        json!(receipt.subscription_id)
    );
    assert_eq!(subscriptions[0]["title"], "Owned audio feed");
    assert_eq!(episodes["items"].as_array().unwrap().len(), 1);
    assert!(episodes["next_cursor"].is_null());
    let episode = &episodes["items"][0];
    assert!(
        !Uuid::parse_str(episode["episode_id"].as_str().unwrap())
            .unwrap()
            .is_nil()
    );
    let recording = &episode["recording"];
    assert_eq!(recording["inherited_title"], fixture::TITLE);
    assert_eq!(recording["artist"], fixture::AUTHOR);
    let recording_id = recording["id"].as_str().unwrap();
    assert!(!Uuid::parse_str(recording_id).unwrap().is_nil());
    let sources_path = format!("/api/v1/recordings/{recording_id}/sources");
    let sources = get(address, token, &sources_path).await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    let source = &sources[0];
    let source_id = source["id"].as_str().unwrap();
    assert!(!Uuid::parse_str(source_id).unwrap().is_nil());
    assert_eq!(source["recording_id"], recording_id);
    assert_eq!(source["provider"], "podcast");
    assert_eq!(source["label"], "Podcast audio");
    assert_eq!(source["ready"], true);
    assert_eq!(source["can_prepare"], false);
    assert!(source["device_id"].is_null());
    for public in [episodes.to_string(), sources.to_string()] {
        assert!(!public.contains(&fixture.enclosure_url));
        assert!(!public.contains("podcast-private-enclosure-canary"));
        assert!(!public.contains("episode.wav"));
    }
    assert_eq!(fixture.feed_requests(), 1);
    assert_eq!(fixture.enclosure_requests(), 0);
    let download_path = format!("/api/v1/recordings/{recording_id}/download");
    assert_eq!(
        call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let audio_path = format!("/api/v1/sources/{source_id}/audio");
    let response = call(address, "GET", &audio_path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "audio/wav");
    assert_eq!(response.headers["accept-ranges"], "bytes");
    assert_eq!(response.body, audio);
    assert_eq!(fixture.enclosure_requests(), 1);
    assert_eq!(
        call(address, "GET", &download_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let credentials = json!({"username": "other", "password": "other podcast runtime password"});
    assert_eq!(
        call(
            address,
            "POST",
            "/api/v1/users",
            Some(credentials.clone()),
            Some(token)
        )
        .await
        .status,
        StatusCode::CREATED
    );
    let login = call(address, "POST", "/api/v1/login", Some(credentials), None).await;
    assert_eq!(login.status, StatusCode::OK);
    let other_session = login.json();
    let other_token = other_session["token"].as_str().unwrap();
    assert_eq!(
        get(address, other_token, "/api/v1/podcasts").await,
        json!([])
    );
    assert_eq!(
        get(address, other_token, "/api/v1/recordings").await["total"],
        0
    );
    for path in [
        "/api/v1/podcasts",
        &episodes_path,
        &sources_path,
        &audio_path,
    ] {
        assert_eq!(
            call(address, "GET", path, None, None).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    for path in [&episodes_path, &sources_path, &audio_path] {
        assert_eq!(
            call(address, "GET", path, None, Some(other_token))
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(fixture.feed_requests(), 1);
    assert_eq!(fixture.enclosure_requests(), 1);
    assert_eq!(fixture.requests(), 2);
    assert_eq!(fixture.unexpected(), 0);
}
