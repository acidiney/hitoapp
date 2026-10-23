#[allow(dead_code)]
#[path = "artist_review_account.rs"]
mod account;
pub use account::{Account, exchange};

use crate::{provider, support};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{Action, Operation};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

pub struct Library {
    pub app: Router,
    pub services: hito_core::Services,
    pub account: Account,
    pub recording: String,
    pub observation: String,
    pub installation_revision: i64,
    playlist: String,
    original: PathBuf,
    original_bytes: Vec<u8>,
    original_metadata: fs::Metadata,
    _directory: tempfile::TempDir,
}

pub async fn exchange_lyrics_deadline(
    app: &Router,
    path: &str,
    body: Value,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, Value) {
    let bytes = serde_json::to_vec(&body).unwrap();
    assert!(bytes.len() <= 4096);
    let mut request = Request::post(path).body(Body::from(bytes)).unwrap();
    *request.headers_mut() = headers;
    tokio::time::timeout(Duration::from_secs(15), async {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 524_288).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, headers, value)
    })
    .await
    .expect("The owned total-budget HTTP exchange must finish within15s")
}

impl Library {
    pub async fn prepare(provider: &provider::Provider) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("owned-music-private-path-canary");
        fs::create_dir(&music).unwrap();
        let original = music.join("owned-original.wav");
        let original_bytes = support::write_wav(&original);
        let mut permissions = fs::metadata(&original).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&original, permissions).unwrap();
        let services = support::services(directory.path(), &music).await;
        let app = hito_server::router(services.clone());
        let account = Account::setup(&app).await;
        let (status, _) = account
            .request(
                &app,
                "PUT",
                "/api/v1/metadata/settings",
                json!({
                    "revision": 0, "enabled": false, "providers": [],
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let libraries = account.get(&app, "/api/v1/libraries").await;
        let shared = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|library| library["personal"] == false)
            .unwrap();
        let (status, _) = account
            .request(
                &app,
                "POST",
                &format!("/api/v1/libraries/{}/scan", shared["id"].as_str().unwrap()),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(services.storage.run_next_job().await.unwrap());
        let indexed = account.get(&app, "/api/v1/recordings").await;
        assert!(indexed["total"] == 1 && indexed["items"][0]["available"] == true);
        let installation = account
            .connect(
                &app,
                &provider.url,
                "Owned lyric provider",
                Some(provider::TOKEN),
            )
            .await;
        assert!(installation["manifest"]["capabilities"] == json!(["playlists", "lyrics"]));
        let (status, imported) = account
            .request(
                &app,
                "POST",
                &format!(
                    "/api/v1/plugins/{}/playlists/import",
                    installation["id"].as_str().unwrap()
                ),
                json!({
                    "request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
                    "playlist_reference": "https://provider.invalid/owned-lyrics-collection",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let playlist = imported["playlist_id"].as_str().unwrap().to_owned();
        let detail = account
            .get(&app, &format!("/api/v1/playlists/{playlist}"))
            .await;
        let entries = detail["entries"].as_array().unwrap();
        assert!(entries.len() == 2 && detail["playlist"]["kind"] == "local");
        let recording = entries[0]["recording_id"].as_str().unwrap().to_owned();
        assert!(
            entries[1]["recording_id"] == recording
                && entries[0]["entry_id"] != entries[1]["entry_id"]
        );
        let recorded = Uuid::parse_str(&recording).unwrap();
        let operations: Vec<_> = [
            Action::RecordingTitleSet {
                recording_id: recorded,
                title: "Owned personal title pin".into(),
            },
            Action::LikeSet {
                recording_id: recorded,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recorded,
                position_ms: 400,
                speed: 1.25,
            },
        ]
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect();
        let (status, receipt) = account
            .request(
                &app,
                "POST",
                "/api/v1/sync",
                json!({"device_id": "owned-lyrics-client", "operations": operations}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            receipt["acknowledged"]
                == json!(
                    operations
                        .iter()
                        .map(|operation| operation.id)
                        .collect::<Vec<_>>()
                )
        );
        let metadata = account
            .get(&app, &format!("/api/v1/recordings/{recording}/metadata"))
            .await;
        assert!(metadata["title"]["personal"] == "Owned personal title pin");
        let observation = &metadata["selected_observation"];
        assert!(
            observation["provider_recording_id"] == provider::PROVIDER_ID
                && observation["provider_revision"] == provider::reference().revision.unwrap()
        );
        assert!(
            observation["plugin_installation_id"] == installation["id"]
                && observation["source_id"].is_null()
        );
        Self {
            app,
            services,
            account,
            recording,
            observation: observation["id"].as_str().unwrap().into(),
            installation_revision: installation["revision"].as_i64().unwrap(),
            playlist,
            original_metadata: fs::metadata(&original).unwrap(),
            original,
            original_bytes,
            _directory: directory,
        }
    }

    pub fn lookup_body(&self) -> Value {
        json!({"observation_id": self.observation, "installation_revision": self.installation_revision})
    }

    pub fn lookup_path(&self) -> String {
        format!("/api/v1/recordings/{}/lyrics", self.recording)
    }

    pub async fn snapshot(&self) -> Value {
        let app = &self.app;
        let account = &self.account;
        let recording = &self.recording;
        let sources = account
            .get(app, &format!("/api/v1/recordings/{recording}/sources"))
            .await;
        assert!(sources == json!([]));
        let (status, download) = account
            .request(
                app,
                "GET",
                &format!("/api/v1/recordings/{recording}/download"),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, progress) = account
            .request(
                app,
                "POST",
                "/api/v1/recordings/progress",
                json!({"recording_ids": [recording]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        json!({
            "catalog": account.get(app, "/api/v1/recordings").await,
            "libraries": account.get(app, "/api/v1/libraries").await,
            "metadata": account.get(app, &format!("/api/v1/recordings/{recording}/metadata")).await,
            "artists": account.get(app, "/api/v1/artists").await,
            "releases": account.get(app, "/api/v1/releases").await,
            "playlist": account.get(app, &format!("/api/v1/playlists/{}", self.playlist)).await,
            "activity": account.get(app, "/api/v1/activity").await,
            "events": account.get(app, "/api/v1/sync?cursor=0").await,
            "plugins": account.get(app, "/api/v1/plugins").await,
            "policy": account.get(app, "/api/v1/metadata/settings").await,
            "sources": sources, "download": download, "progress": progress,
        })
    }

    pub fn assert_original(&self) {
        assert!(fs::read(&self.original).unwrap() == self.original_bytes);
        let metadata = fs::metadata(&self.original).unwrap();
        assert!(metadata.len() == self.original_metadata.len());
        assert!(metadata.modified().unwrap() == self.original_metadata.modified().unwrap());
        assert!(
            metadata.permissions().readonly() && self.original_metadata.permissions().readonly()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert!(
                metadata.ino() == self.original_metadata.ino()
                    && metadata.mode() == self.original_metadata.mode()
            );
        }
    }
}
