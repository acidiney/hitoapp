#![cfg(feature = "device")]
#[path = "fixtures/application.rs"]
mod application;
#[path = "../../hito-audio/tests/support/capture.rs"]
#[allow(dead_code)]
mod capture;
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn the_protected_desktop_api_drives_native_output_controls_progress_and_joined_shutdown() {
    let mut app = application::Application::start().await;
    let inspected = app.call("GET", "api/v1/player", Value::Null).await;
    assert_eq!(inspected["native"], true);
    assert!(inspected["state"].is_null());
    let page = app
        .request("GET", "library", Value::Null)
        .await
        .text()
        .await
        .unwrap();
    assert!(page.contains("\"native_playback\":true"));
    assert_eq!(
        app.client
            .get(app.root.join("api/v1/player").unwrap())
            .bearer_auth(&app.token)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let attached = app
        .call(
            "POST",
            "api/v1/player/claim",
            json!({"epoch":inspected["epoch"]}),
        )
        .await;
    let lease = &attached["lease"];
    let mut state = app.observe(lease, "state").await;
    let mut position = app.observe(lease, "position").await;
    let capture = capture::Capture::open();
    app.command(
        lease,
        1,
        0,
        json!({"kind":"replace", "recordings":[app.recording], "start":0}),
    )
    .await;
    app.command(lease, 2, 1, json!({"kind":"resume"})).await;
    state.until(|state| state["playing"] == true).await;
    position
        .until(|position| position["position_ms"].as_u64().unwrap() >= 150)
        .await;
    assert!(capture::energy(&capture.recent(0.05)) > 0.00001);
    app.command(lease, 3, 1, json!({"kind":"pause"})).await;
    state
        .until(|state| {
            state["phase"] == "ready"
                && state["playing"] == false
                && state["desired_playing"] == false
        })
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let activity = app.call("GET", "api/v1/activity", Value::Null).await;
            if activity["history"][0]["position_ms"]
                .as_u64()
                .is_some_and(|position| position >= 150)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let seek = app
        .command(lease, 4, 1, json!({"kind":"seek", "position_ms":0}))
        .await;
    position
        .until(|position| position["seek_id"] == seek["id"])
        .await;
    let previous = app.command(lease, 5, 1, json!({"kind":"previous"})).await;
    assert_eq!(previous["revision"], 1);
    position
        .until(|position| position["seek_id"] == previous["id"])
        .await;
    app.command(lease, 6, 1, json!({"kind":"shuffle"})).await;
    state.until(|state| state["shuffle"] == true).await;
    for (sequence, mode) in [(7, "all"), (8, "one"), (9, "off")] {
        let receipt = app
            .command(lease, sequence, 1, json!({"kind":"repeat"}))
            .await;
        assert_eq!(receipt["revision"], 1);
        let observed = state.until(|state| state["repeat"] == mode).await;
        assert_eq!(observed["desired_playing"], false);
    }
    app.command(lease, 10, 1, json!({"kind":"speed", "rate":2000}))
        .await;
    state
        .until(|state| state["speed"] == 2000 && state["desired_playing"] == false)
        .await;
    app.command(lease, 11, 1, json!({"kind":"volume", "level":0}))
        .await;
    app.command(lease, 12, 1, json!({"kind":"resume"})).await;
    state
        .until(|state| state["playing"] == true && state["volume"] == 0)
        .await;
    capture.after(capture.mark(), 0.2);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    app.command(lease, 13, 1, json!({"kind":"volume", "level":750}))
        .await;
    capture.after(capture.mark(), 0.2);
    assert!(capture::energy(&capture.recent(0.1)) > 0.00001);
    app.host.take().unwrap().shutdown().await.unwrap();
    capture.after(capture.mark(), 0.3);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    while position.next().await.is_some() {}
    while state.next().await.is_some() {}
    assert!(
        app.client
            .get(app.root.join("health").unwrap())
            .send()
            .await
            .is_err()
    );
    let replacement = hito_desktop::LocalHost::start(app.paths()).await.unwrap();
    replacement.shutdown().await.unwrap();
}
