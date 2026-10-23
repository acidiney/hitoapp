use hito_core::test_support::{
    metadata_tag_logs::CapturedLogs, podcast_audio_service::PodcastAudioServer,
};
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::{CoreConfig, HostDeviceId, PodcastNetworkConfig};
use serde_json::json;
use support::call;
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const JOINED: &str = "Podcast episodes joined with saved progress";

#[tokio::test]
async fn only_a_progress_reload_reads_saved_progress_for_the_episode_page() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-partial.wav"));
    let mut fixture = PodcastAudioServer::start(wav).await;
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("podcast-partial-host").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![fixture.origin.clone()]).unwrap();
    let services = hito_core::open(config).await.unwrap();
    let app = hito_server::router(services.clone());
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/setup",
        json!({"username": "podcast-partial-owner", "password": "owned partial page password"}),
        None,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let (_, receipt) = call(
        &app,
        "POST",
        "/api/v1/podcasts",
        json!({"request_id": Uuid::new_v4(), "feed_url": fixture.feed_url}),
        Some(token),
    )
    .await;
    assert!(
        services
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let path = format!("/podcasts/{}", receipt["subscription_id"].as_str().unwrap());

    let episodes = CapturedLogs::default();
    let status = reload(&app, &path, token, "episodes")
        .with_subscriber(episodes.subscriber())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!episodes.text().contains(JOINED));

    let progress = CapturedLogs::default();
    let status = reload(&app, &path, token, "progress")
        .with_subscriber(progress.subscriber())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(progress.text().contains(JOINED));
    assert!(fixture.finish().await);
}

async fn reload(app: &Router, path: &str, token: &str, fields: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("cookie", format!("hito_session={token}"))
                .header("x-inertia", "true")
                .header("x-inertia-partial-component", "PodcastPage")
                .header("x-inertia-partial-data", fields)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
