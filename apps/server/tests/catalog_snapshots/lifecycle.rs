use super::fixture::{Fixture, ROOT, tag_wav};
use axum::http::StatusCode;
use hito_core::test_support::support::write_wav;
use serde_json::{Value, json};

#[tokio::test]
async fn a_lost_response_resumes_immutable_facts_until_release_then_refreshes() {
    let f = Fixture::start().await;
    let path = f.music.join("Owned.wav");
    write_wav(&path, 0);
    tag_wav(&path, "First title");
    f.scan().await;
    let recording = f
        .call("GET", "/api/v1/recordings", Value::Null)
        .await
        .json()["items"][0]["id"]
        .clone();
    let playlist = uuid::Uuid::new_v4();
    let operations = vec![
        json!({"id":uuid::Uuid::new_v4(), "action":{"kind":"playlist_create", "playlist_id":playlist, "name":"Private collection sentinel"}}),
        json!({"id":uuid::Uuid::new_v4(), "action":{"kind":"entry_insert", "playlist_id":playlist, "entry_id":uuid::Uuid::new_v4(), "recording_id":recording, "position":0}}),
        json!({"id":uuid::Uuid::new_v4(), "action":{"kind":"entry_insert", "playlist_id":playlist, "entry_id":uuid::Uuid::new_v4(), "recording_id":recording, "position":1}}),
        json!({"id":uuid::Uuid::new_v4(), "action":{"kind":"like_set", "recording_id":recording, "liked":true}}),
        json!({"id":uuid::Uuid::new_v4(), "action":{"kind":"progress_set", "recording_id":recording, "position_ms":700, "speed":1.25}}),
    ];
    assert_eq!(
        f.call(
            "POST",
            "/api/v1/sync",
            json!({"device_id":"snapshot-http-client", "operations":operations})
        )
        .await
        .status,
        StatusCode::OK
    );
    let playlist_path = format!("/api/v1/playlists/{playlist}");
    let private_playlist = f.call("GET", &playlist_path, Value::Null).await.json();
    assert_eq!(private_playlist["entries"].as_array().unwrap().len(), 2);
    let before = f
        .call("GET", "/api/v1/sync?cursor=0", Value::Null)
        .await
        .json();
    let history = f.call("GET", "/api/v1/activity", Value::Null).await.json();
    assert_eq!(history["history"][0]["position_ms"], 700);
    let snapshot = f.begin().await;
    assert_eq!(snapshot["collection_watermark"], before["cursor"]);
    let id = snapshot["id"].as_str().unwrap();
    let page_path = format!("{ROOT}/{id}/pages/0");
    let first = f.call("GET", &page_path, Value::Null).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.json()["recordings"][0]["title"], "First title");
    tag_wav(&path, "Changed after publication");
    f.scan().await;
    let rescanned_history = f.call("GET", "/api/v1/activity", Value::Null).await.json();
    assert_eq!(
        rescanned_history["history"].as_array().unwrap().len(),
        history["history"].as_array().unwrap().len()
    );
    for key in ["id", "position_ms", "speed", "updated_at", "liked"] {
        assert_eq!(
            rescanned_history["history"][0][key],
            history["history"][0][key]
        );
    }
    assert_eq!(
        f.begin().await,
        snapshot,
        "Retry must not change metadata or extend expiry"
    );
    assert_eq!(
        f.call("GET", &page_path, Value::Null).await.bytes,
        first.bytes
    );
    let release = f.call("DELETE", &format!("{ROOT}/{id}"), Value::Null).await;
    assert_eq!(release.status, StatusCode::NO_CONTENT);
    assert!(release.bytes.is_empty());
    release.private();
    assert_eq!(
        f.call("DELETE", &format!("{ROOT}/{id}"), Value::Null)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call("GET", &page_path, Value::Null).await.status,
        StatusCode::NOT_FOUND
    );
    let fresh = f.begin().await;
    assert_ne!(fresh["id"], snapshot["id"]);
    assert_eq!(fresh["collection_watermark"], before["cursor"]);
    let page = f
        .call(
            "GET",
            &format!("{ROOT}/{}/pages/0", fresh["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(
        page.json()["recordings"][0]["title"],
        "Changed after publication"
    );
    assert_eq!(
        f.call("GET", "/api/v1/sync?cursor=0", Value::Null)
            .await
            .json(),
        before
    );
    assert_eq!(
        f.call("GET", "/api/v1/activity", Value::Null).await.json(),
        rescanned_history
    );
    let after_playlist = f.call("GET", &playlist_path, Value::Null).await.json();
    assert_eq!(after_playlist["playlist"], private_playlist["playlist"]);
    assert_eq!(after_playlist["entries"].as_array().unwrap().len(), 2);
    for (before, after) in private_playlist["entries"]
        .as_array()
        .unwrap()
        .iter()
        .zip(after_playlist["entries"].as_array().unwrap())
    {
        for key in ["entry_id", "recording_id", "position"] {
            assert_eq!(before[key], after[key]);
        }
    }
    f.services.catalog_replication.close().await.unwrap();
}
