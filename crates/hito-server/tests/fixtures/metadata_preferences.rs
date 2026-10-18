#[allow(dead_code)]
#[path = "../../../hito-core/tests/fixtures/artist_catalog_profiles_service.rs"]
mod provider;

use crate::support::{call, indexed_app};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
pub(super) use provider::{CatalogServer, Counts};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

pub(super) struct Account {
    pub(super) token: String,
    pub(super) id: String,
    playlist: String,
}

impl Account {
    pub(super) async fn request(
        &self,
        app: &Router,
        method: &str,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("cookie", format!("hito_session={}", self.token))
                    .header("x-hito-account", self.id.as_str())
                    .header("x-hito-request", "1")
                    .header("origin", "http://localhost:8080")
                    .header("content-type", "application/json")
                    .body(if body.is_null() {
                        Body::empty()
                    } else {
                        Body::from(body.to_string())
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    pub(super) async fn get(&self, app: &Router, path: &str) -> Value {
        let (status, body) = self.request(app, "GET", path, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        body
    }

    async fn seed(&self, app: &Router, recording: &str, label: &str) {
        let operations = [
            json!({"kind": "playlist_create", "playlist_id": self.playlist, "name": label}),
            json!({"kind": "entry_insert", "playlist_id": self.playlist, "entry_id": Uuid::new_v4(), "recording_id": recording, "position": 0}),
            json!({"kind": "entry_insert", "playlist_id": self.playlist, "entry_id": Uuid::new_v4(), "recording_id": recording, "position": 1}),
            json!({"kind": "recording_title_set", "recording_id": recording, "title": label}),
            json!({"kind": "recording_artist_set", "recording_id": recording, "artist": label}),
            json!({"kind": "like_set", "recording_id": recording, "liked": true}),
            json!({"kind": "progress_set", "recording_id": recording, "position_ms": 400, "speed": 1.25}),
        ].map(|action| json!({"id": Uuid::new_v4(), "action": action}));
        let (status, _) = self
            .request(
                app,
                "POST",
                "/api/v1/sync",
                json!({"device_id": "owned-preferences-client", "operations": operations}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn state(&self, app: &Router, recording: &str) -> Value {
        let (status, progress) = self
            .request(
                app,
                "POST",
                "/api/v1/recordings/progress",
                json!({"recording_ids": [recording]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        json!({
            "libraries": self.get(app, "/api/v1/libraries").await,
            "recordings": self.get(app, "/api/v1/recordings").await,
            "artists": self.get(app, "/api/v1/artists").await,
            "releases": self.get(app, "/api/v1/releases").await,
            "metadata": self.get(app, &format!("/api/v1/recordings/{recording}/metadata")).await,
            "sources": self.get(app, &format!("/api/v1/recordings/{recording}/sources")).await,
            "playlist": self.get(app, &format!("/api/v1/playlists/{}", self.playlist)).await,
            "activity": self.get(app, "/api/v1/activity").await,
            "events": self.get(app, "/api/v1/sync?cursor=0").await,
            "progress": progress,
        })
    }
}

pub(super) struct Library {
    pub(super) app: Router,
    pub(super) accounts: [Account; 2],
    recording: String,
    _directory: tempfile::TempDir,
}

impl Library {
    pub(super) async fn prepare() -> Self {
        let (directory, app, token, recording) = indexed_app().await;
        let (status, admin) = call(&app, "GET", "/api/v1/me", Value::Null, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let admin = Account {
            token,
            id: admin["id"].as_str().unwrap().into(),
            playlist: Uuid::new_v4().to_string(),
        };
        let (status, user) = admin
            .request(
                &app,
                "POST",
                "/api/v1/users",
                json!({"username": "preferences-listener", "password": "owned preferences listener password"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let libraries = admin.get(&app, "/api/v1/libraries").await;
        let shared = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|library| library["personal"] == false)
            .expect("The owned indexed fixture must provide a shared library");
        let (status, _) = admin
            .request(
                &app,
                "PUT",
                &format!(
                    "/api/v1/libraries/{}/members/{}",
                    shared["id"].as_str().unwrap(),
                    user["id"].as_str().unwrap()
                ),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, session) = call(
            &app,
            "POST",
            "/api/v1/login",
            json!({"username": "preferences-listener", "password": "owned preferences listener password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let other = Account {
            token: session["token"].as_str().unwrap().into(),
            id: user["id"].as_str().unwrap().into(),
            playlist: Uuid::new_v4().to_string(),
        };
        admin
            .seed(&app, &recording, "Private administrator credit")
            .await;
        other
            .seed(&app, &recording, "Private listener credit")
            .await;
        Self {
            app,
            accounts: [admin, other],
            recording,
            _directory: directory,
        }
    }

    pub(super) async fn state(&self) -> Value {
        json!([
            self.accounts[0].state(&self.app, &self.recording).await,
            self.accounts[1].state(&self.app, &self.recording).await,
        ])
    }
}
