use super::playlist_fixture;
use super::support::{admin, app, call};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn playlist_import_returns_an_editable_copy_with_ordered_retained_occurrences() {
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let provider = playlist_fixture::connect(&app, &token).await;
    let request_id = Uuid::new_v4();
    let (status, receipt) = call(
        &app,
        "POST",
        &provider.import_path,
        json!({
            "request_id": request_id,
            "installation_revision": provider.revision,
            "playlist_reference": "https://provider.invalid/playlists/pasted-reference"
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["request_id"], request_id.to_string());
    assert_eq!(
        receipt
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["playlist_id", "request_id"]
    );
    let playlist_id = receipt["playlist_id"].as_str().unwrap();
    Uuid::parse_str(playlist_id).unwrap();
    let (status, detail) = call(
        &app,
        "GET",
        &format!("/api/v1/playlists/{playlist_id}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["playlist"]["kind"], "local");
    assert_eq!(detail["playlist"]["name"], "Provider selection");
    assert_eq!(
        detail["origin"]["attribution"]["label"],
        "Curated by the fixture owner"
    );
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "Known recording",
            "Withdrawn recording",
            "Unknown provider item",
            "Known recording"
        ]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["position"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| Uuid::parse_str(entry["entry_id"].as_str().unwrap()).unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    assert_eq!(entries[0]["recording_id"], entries[3]["recording_id"]);
    let recording_id = entries[0]["recording_id"].as_str().unwrap();
    Uuid::parse_str(recording_id).unwrap();
    assert!(entries[1]["recording_id"].is_null());
    assert!(entries[2]["recording_id"].is_null());
    assert_eq!(entries[1]["origin"]["kind"], "unavailable");
    assert_eq!(entries[2]["origin"]["kind"], "unknown");
    assert!(entries.iter().all(|entry| entry["available"] == false));
    let (status, sources) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording_id}/sources"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sources, json!([]));
}
