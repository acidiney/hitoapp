use crate::artwork_normalization_provider as provider;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{
    CoreConfig, HostDeviceId, Services,
    test_support::{database_owner::DatabaseOwner, support},
};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap()
    }
}

pub struct Context {
    pub owner: String,
    pub peer: String,
    pub recording: String,
    pub release: String,
    copies: [String; 2],
}

pub struct Library {
    directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    services: Option<Services>,
    app: Option<Router>,
    provider: Option<FakePlugin>,
    original_audio: Vec<u8>,
}

impl Library {
    pub fn retain() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let database = DatabaseOwner::retain(directory.clone());
        Self {
            directory,
            database,
            services: None,
            app: None,
            provider: None,
            original_audio: Vec::new(),
        }
    }

    pub async fn open(&mut self) {
        let music = self.directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        self.original_audio = support::write_wav(&music.join("Owned.wav"), 0);
        std::fs::write(music.join("cover.png"), provider::COVER).unwrap();
        let mut config = CoreConfig::new(
            self.directory.path().join("hito.db"),
            music,
            HostDeviceId::new("artwork-normalization").unwrap(),
        );
        self.database.attach(&mut config);
        self.services = Some(hito_core::open(config).await.unwrap());
        self.app = Some(hito_server::router(self.hito().clone()));
        self.provider = Some(provider::start().await);
        let decoded = image::load_from_memory(provider::COVER).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (1024, 768));
        assert_ne!(decoded.get_pixel(32, 32), decoded.get_pixel(500, 350));
        assert_ne!(decoded.get_pixel(500, 350), decoded.get_pixel(700, 590));
    }

    pub fn hito(&self) -> &Services {
        self.services.as_ref().unwrap()
    }
    pub fn provider(&self) -> &FakePlugin {
        self.provider.as_ref().unwrap()
    }

    pub async fn exchange(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Value,
    ) -> Response {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("x-hito-request", "1");
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = self
                .app
                .as_ref()
                .unwrap()
                .clone()
                .oneshot(request.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec();
            Response {
                status,
                headers,
                bytes,
            }
        })
        .await
        .expect("The owned artwork HTTP request must finish")
    }

    pub async fn get(&self, token: &str, path: &str) -> Value {
        let response = self.exchange(Some(token), "GET", path, Value::Null).await;
        assert_eq!(response.status, StatusCode::OK);
        response.json()
    }

    pub async fn prepare(&self) -> Context {
        let setup = self
            .exchange(
                None,
                "POST",
                "/api/v1/setup",
                json!({
            "username": "owner", "password": "owned artwork normalization owner password" }),
            )
            .await;
        assert_eq!(setup.status, StatusCode::CREATED);
        let owner = setup.json()["token"].as_str().unwrap().to_owned();
        let offered = self
            .exchange(
                Some(&owner),
                "POST",
                "/api/v1/plugins/services",
                json!({
            "name": "Owned artwork normalization", "base_url": self.provider().url }),
            )
            .await;
        assert_eq!(offered.status, StatusCode::CREATED);
        let offered = offered.json();
        let installed = self
            .exchange(
                Some(&owner),
                "POST",
                "/api/v1/plugins",
                json!({
            "service_id": offered["id"], "service_revision": offered["revision"] }),
            )
            .await;
        assert_eq!(installed.status, StatusCode::CREATED);
        let installed = installed.json();
        let mut copies = Vec::new();
        for _ in 0..2 {
            let imported = self
                .exchange(
                    Some(&owner),
                    "POST",
                    &format!(
                        "/api/v1/plugins/{}/playlists/import",
                        installed["id"].as_str().unwrap()
                    ),
                    json!({
                    "request_id": Uuid::new_v4(), "installation_revision": installed["revision"],
                    "playlist_reference": "https://provider.invalid/owned-normalization" }),
                )
                .await;
            assert_eq!(imported.status, StatusCode::OK);
            copies.push(imported.json()["playlist_id"].as_str().unwrap().to_owned());
        }
        assert_ne!(copies[0], copies[1]);
        let recordings = self.get(&owner, "/api/v1/recordings").await;
        assert_eq!(recordings["total"], 1);
        let recording = recordings["items"][0]["id"].as_str().unwrap().to_owned();
        let release = recordings["items"][0]["release_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let operations = [
            json!({"kind": "recording_title_set", "recording_id": recording, "title": "My owned recording"}),
            json!({"kind": "like_set", "recording_id": recording, "liked": true}),
            json!({"kind": "progress_set", "recording_id": recording, "position_ms": 4200, "speed": 1.25}),
        ].into_iter().map(|action| json!({"id": Uuid::new_v4(), "action": action})).collect::<Vec<_>>();
        let accepted = self
            .exchange(
                Some(&owner),
                "POST",
                "/api/v1/sync",
                json!({
            "device_id": "owned-artwork-normalization", "operations": operations }),
            )
            .await;
        assert_eq!(accepted.status, StatusCode::OK);
        assert_eq!(
            accepted.json()["acknowledged"],
            json!(
                operations
                    .iter()
                    .map(|op| op["id"].clone())
                    .collect::<Vec<_>>()
            )
        );
        let credentials =
            json!({"username": "peer", "password": "owned artwork normalization peer password"});
        let created = self
            .exchange(Some(&owner), "POST", "/api/v1/users", credentials.clone())
            .await;
        assert_eq!(created.status, StatusCode::CREATED);
        let logged = self
            .exchange(None, "POST", "/api/v1/login", credentials)
            .await;
        assert_eq!(logged.status, StatusCode::OK);
        let peer = logged.json()["token"].as_str().unwrap().to_owned();
        Context {
            owner,
            peer,
            recording,
            release,
            copies: copies.try_into().unwrap(),
        }
    }

    pub async fn snapshot(&self, context: &Context) -> Value {
        let progress = self
            .exchange(
                Some(&context.owner),
                "POST",
                "/api/v1/recordings/progress",
                json!({"recording_ids": [context.recording]}),
            )
            .await;
        assert_eq!(progress.status, StatusCode::OK);
        let mut copies = Vec::new();
        for id in &context.copies {
            let copy = self
                .get(&context.owner, &format!("/api/v1/playlists/{id}"))
                .await;
            assert_eq!(copy["entries"].as_array().unwrap().len(), 2);
            assert!(
                copy["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|entry| entry["recording_id"] == context.recording)
            );
            copies.push(copy);
        }
        json!({
            "recordings": self.get(&context.owner, "/api/v1/recordings").await,
            "metadata": self.get(&context.owner, &format!("/api/v1/recordings/{}/metadata", context.recording)).await,
            "sources": self.get(&context.owner, &format!("/api/v1/recordings/{}/sources", context.recording)).await,
            "copies": copies, "progress": progress.json(),
            "events": self.get(&context.owner, "/api/v1/sync?cursor=0").await,
            "peer_recordings": self.get(&context.peer, "/api/v1/recordings").await,
            "peer_playlists": self.get(&context.peer, "/api/v1/playlists").await,
            "peer_plugins": self.get(&context.peer, "/api/v1/plugins").await,
            "peer_events": self.get(&context.peer, "/api/v1/sync?cursor=0").await,
        })
    }

    pub fn assert_originals(&self) {
        let music = self.directory.path().join("music");
        assert_eq!(
            Sha256::digest(std::fs::read(music.join("Owned.wav")).unwrap()),
            Sha256::digest(&self.original_audio)
        );
        assert_eq!(
            Sha256::digest(std::fs::read(music.join("cover.png")).unwrap()),
            Sha256::digest(provider::COVER)
        );
    }

    pub async fn close(mut self) {
        let provider_joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        drop(self.app.take());
        let closed = if let Some(hito) = self.services.take() {
            let playback = hito.playback.close().await;
            let replicas = hito.replicas.close().await;
            let replication = hito.catalog_replication.close().await;
            drop(hito);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
        let retired = self.database.retire(Duration::from_secs(3)).await;
        assert!(provider_joined && closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
    }
}
