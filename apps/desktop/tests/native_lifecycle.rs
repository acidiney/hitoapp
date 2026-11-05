#![cfg(feature = "device")]
#[path = "fixtures/application.rs"]
mod application;
#[path = "fixtures/events.rs"]
mod events;
use hito_desktop::{Error, LocalHost};
use serde_json::{Value, json};
use std::time::Duration;
use tracing::instrument::WithSubscriber;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn canceled_shutdown_retains_the_library_until_native_progress_and_http_cleanup_join() {
    let events = events::Events::default();
    let writer = events.clone();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(tracing_subscriber::fmt()
        .json().with_ansi(false)
        .with_env_filter("hito_core=trace,hito_desktop=trace,hito_server=trace,hito_playback=trace,hito_audio=trace")
        .with_writer(move || writer.clone()).finish());
    let mut app = application::Application::start()
        .with_subscriber(dispatch)
        .await;
    let inspected = app.call("GET", "api/v1/player", Value::Null).await;
    let attached = app
        .call(
            "POST",
            "api/v1/player/claim",
            json!({"epoch":inspected["epoch"]}),
        )
        .await;
    let lease = &attached["lease"];
    let rejected = app.request("POST", "api/v1/player/commands", json!({
        "id":uuid::Uuid::new_v4(), "lease":lease, "sequence":1, "revision":0,
        "action":{"kind":"replace", "recordings":[app.recording], "start":0, "source_url":"https://private.invalid/RAW_SOURCE_SENTINEL?token=private"}
    })).await;
    assert_eq!(rejected.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        !rejected
            .text()
            .await
            .unwrap()
            .contains("RAW_SOURCE_SENTINEL")
    );
    let mut position = app.observe(lease, "position").await;
    app.command(
        lease,
        1,
        0,
        json!({"kind":"replace", "recordings":[app.recording], "start":0}),
    )
    .await;
    app.command(lease, 2, 1, json!({"kind":"resume"})).await;
    position
        .until(|position| position["position_ms"].as_u64().unwrap() >= 150)
        .await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let activity = app.call("GET", "api/v1/activity", Value::Null).await;
            if !activity["history"].as_array().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(app.directory.path().join("data/hito.db")),
    )
    .await
    .unwrap();
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let before = events.count("player_progress_write_started");
    app.command(lease, 3, 1, json!({"kind":"pause"})).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        events.after("player_progress_write_started", before),
    )
    .await
    .unwrap();
    let mut closing = tokio::spawn(app.host.take().unwrap().shutdown());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut closing)
            .await
            .is_err()
    );
    closing.abort();
    assert!(closing.await.unwrap_err().is_cancelled());
    assert!(matches!(
        LocalHost::start(app.paths()).await,
        Err(Error::AlreadyRunning)
    ));
    transaction.rollback().await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        events.after("desktop_backend_joined", 0),
    )
    .await
    .unwrap();
    let replacement = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match LocalHost::start(app.paths()).await {
                Ok(host) => break host,
                Err(Error::AlreadyRunning) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(error) => panic!("Replacement failed: {error}"),
            }
        }
    })
    .await
    .unwrap();
    let bootstrapped = app
        .client
        .get(replacement.entry_url())
        .send()
        .await
        .unwrap();
    let cookie = bootstrapped.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let activity: Value = app
        .client
        .get(replacement.entry_url().join("/api/v1/activity").unwrap())
        .header("cookie", cookie)
        .bearer_auth(&app.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(activity["history"][0]["position_ms"].as_u64().unwrap() >= 150);
    replacement.shutdown().await.unwrap();
    pool.close().await;
    let output = events.text();
    for private in [
        &app.token,
        &app.cookie,
        lease.as_str().unwrap(),
        app.directory.path().to_str().unwrap(),
        "private native desktop password",
        "RAW_SOURCE_SENTINEL",
    ] {
        assert!(!output.contains(private), "Private input appeared in logs");
    }
    assert!(
        events.count("http_response") > 0,
        "Network HTTP work must retain its subscriber"
    );
    let frame = tokio::time::timeout(
        Duration::from_secs(2),
        events.after("player_observation_closed", 0),
    )
    .await
    .unwrap();
    assert!(
        frame["spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span.get("request_id").is_some())
    );
}
