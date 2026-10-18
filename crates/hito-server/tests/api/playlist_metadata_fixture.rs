use crate::{
    playlist_fixture::{self, PlaylistProvider},
    support::{admin, app, call},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::ImportProviderPlaylist;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

pub(super) struct MetadataCopy {
    pub app: Router,
    pub token: String,
    pub provider: PlaylistProvider,
    pub playlist: String,
    pub recording: String,
    pub import_body: Value,
    pub import_receipt: Value,
    _directory: tempfile::TempDir,
}

impl MetadataCopy {
    pub async fn start() -> Self {
        let (directory, app) = app().await;
        let token = admin(&app).await;
        let provider = playlist_fixture::connect(&app, &token).await;
        let import_body = serde_json::to_value(ImportProviderPlaylist {
            request_id: Uuid::new_v4(),
            installation_revision: provider.revision,
            playlist_reference: "https://provider.invalid/playlists/metadata-copy".into(),
        })
        .unwrap();
        let (status, import_receipt) = call(
            &app,
            "POST",
            &provider.import_path,
            import_body.clone(),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let playlist = import_receipt["playlist_id"].as_str().unwrap().to_owned();
        let (status, copy) = call(
            &app,
            "GET",
            &format!("/api/v1/playlists/{playlist}"),
            Value::Null,
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let recording = copy["entries"][0]["recording_id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            app,
            token,
            provider,
            playlist,
            recording,
            import_body,
            import_receipt,
            _directory: directory,
        }
    }

    pub async fn get(&self, path: &str) -> Value {
        let (status, body) = call(&self.app, "GET", path, Value::Null, Some(&self.token)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    pub async fn state(&self) -> Value {
        let recording = &self.recording;
        json!({
            "copy": self.get(&format!("/api/v1/playlists/{}", self.playlist)).await,
            "catalog": self.get("/api/v1/recordings").await,
            "albums": self.get("/api/v1/releases").await,
            "artists": self.get("/api/v1/artists").await,
            "metadata": self.get(&format!("/api/v1/recordings/{recording}/metadata")).await,
            "activity": self.get("/api/v1/activity").await,
            "sources": self.get(&format!("/api/v1/recordings/{recording}/sources")).await,
            "source_choice": self.get(&format!("/api/v1/recordings/{recording}/source-choice")).await,
            "events": self.get("/api/v1/sync?cursor=0").await,
        })
    }

    pub async fn refresh(&self, body: &Value) -> (StatusCode, Vec<u8>) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/playlists/{}/metadata/refresh",
                        self.playlist
                    ))
                    .header("content-type", "application/json")
                    .header("x-hito-request", "1")
                    .header("authorization", format!("Bearer {}", self.token))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, bytes.to_vec())
    }

    pub async fn publish_verified_album(&self) {
        self.provider
            .server
            .reply(
                "v1/playlists/detail",
                json!({
                    "id": "playlist/retained", "name": "Changed upstream selection",
                    "snapshot": "snapshot-with-album", "total_count": 3,
                    "attribution": {"label": "Verified metadata fixture"}
                }),
            )
            .await;
        let known = json!({"kind": "recording", "recording": {
            "id": "recording/known", "title": "Verified recording", "artist": "Verified performer",
            "duration_ms": 124_000, "revision": "verified-album-one",
            "release": {"id": "release/verified", "title": "Verified provider album"},
            "disc_number": 1, "track_number": 4
        }});
        self.provider
            .server
            .reply(
                "v1/playlists/items",
                json!({
                    "playlist_id": "playlist/retained", "snapshot": "snapshot-with-album",
                    "items": [
                        {"occurrence_id": "duplicate", "item": known},
                        {"occurrence_id": "new-upstream", "item": {"kind": "recording", "recording": {
                            "id": "recording/new", "title": "New upstream recording", "artist": "New artist"
                        }}},
                        {"occurrence_id": "first", "item": known}
                    ],
                    "next_cursor": null
                }),
            )
            .await;
    }

    pub async fn provider_requests(&self) -> [usize; 8] {
        [
            self.provider.server.requests("v1/manifest").await.len(),
            self.provider
                .server
                .requests("v1/playlists/detail")
                .await
                .len(),
            self.provider
                .server
                .requests("v1/playlists/items")
                .await
                .len(),
            self.provider.server.requests("v1/search").await.len(),
            self.provider
                .server
                .requests("v1/recordings/detail")
                .await
                .len(),
            self.provider
                .server
                .requests("v1/recordings/metadata")
                .await
                .len(),
            self.provider.server.requests("v1/sources").await.len(),
            self.provider.server.requests("v1/audio").await.len(),
        ]
    }
}

pub(super) fn copy_identity(copy: &Value) -> Value {
    json!({
        "playlist": copy["playlist"],
        "origin": copy["origin"],
        "entries": copy["entries"].as_array().unwrap().iter().map(|entry| json!({
            "entry_id": entry["entry_id"], "recording_id": entry["recording_id"],
            "position": entry["position"], "origin": entry["origin"]
        })).collect::<Vec<_>>()
    })
}

pub(super) fn listening_state(activity: &Value) -> Value {
    json!(
        activity["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| json!({
                "id": entry["id"], "position_ms": entry["position_ms"],
                "speed": entry["speed"], "updated_at": entry["updated_at"], "liked": entry["liked"]
            }))
            .collect::<Vec<_>>()
    )
}
