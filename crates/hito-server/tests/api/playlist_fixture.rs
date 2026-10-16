use super::provider_fixture::ProviderServer;
use super::support::call;
use axum::{Router, http::StatusCode};
use serde_json::json;

pub(super) struct PlaylistProvider {
    pub server: ProviderServer,
    pub import_path: String,
    pub revision: i64,
}

pub(super) async fn connect(app: &Router, token: &str) -> PlaylistProvider {
    let server = ProviderServer::start().await;
    server
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
    server
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
    server
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
    let (status, service) = call(
        app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Playlist collection", "base_url": server.url}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(
        app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    PlaylistProvider {
        server,
        import_path: format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        revision: installation["revision"].as_i64().unwrap(),
    }
}
