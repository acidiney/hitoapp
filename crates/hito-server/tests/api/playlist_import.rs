use super::provider_fixture as fixture;
use super::support::{admin, app, call};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn playlist_import_returns_an_editable_copy_with_ordered_retained_occurrences() {
    let provider = fixture::ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.http-playlist-fixture",
                "name": "HTTP playlist fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"]
            }),
        )
        .await;
    provider
        .reply(
            "v1/playlists/detail",
            json!({
                "id": "playlist/retained",
                "name": "Provider selection",
                "snapshot": "snapshot-one",
                "total_count": 4,
                "attribution": {"label": "Curated by the fixture owner"}
            }),
        )
        .await;
    let known = json!({
        "kind": "recording",
        "recording": {
            "id": "recording/known",
            "title": "Known recording",
            "artist": "Fixture artist",
            "duration_ms": 123_000
        }
    });
    provider
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": "playlist/retained",
                "snapshot": "snapshot-one",
                "items": [
                    {"occurrence_id": "first", "item": known},
                    {"occurrence_id": "withdrawn", "item": {
                        "kind": "unavailable", "provider_recording_id": "recording/withdrawn",
                        "title": "Withdrawn recording", "artist": "Retained artist"
                    }},
                    {"occurrence_id": "unknown", "item": {"kind": "unknown"}},
                    {"occurrence_id": "duplicate", "item": known}
                ],
                "next_cursor": null
            }),
        )
        .await;
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let (status, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Playlist collection", "base_url": provider.url}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let request_id = Uuid::new_v4();
    let (status, receipt) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "request_id": request_id,
            "installation_revision": installation["revision"],
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
