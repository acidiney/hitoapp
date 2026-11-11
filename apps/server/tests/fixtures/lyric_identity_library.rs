use crate::{account::Account, support};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
pub struct Library {
    pub app: Router,
    pub services: hito_core::Services,
    pub account: Account,
    pub recording: String,
    original: PathBuf,
    bytes: Vec<u8>,
    metadata: fs::Metadata,
    _directory: tempfile::TempDir,
}
impl Library {
    pub async fn prepare() -> Self {
        Self::prepare_with_duration(1).await
    }

    pub async fn prepare_with_duration(seconds: u32) -> Self {
        assert!((1..=180).contains(&seconds));
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("owned-local-music");
        fs::create_dir(&music).unwrap();
        let original = music.join("owned.wav");
        let mut bytes = support::write_wav(&original);
        if seconds > 1 {
            let pcm = bytes[44..].to_vec();
            for _ in 1..seconds {
                bytes.extend_from_slice(&pcm);
            }
            let riff_length = (bytes.len() - 8) as u32;
            let data_length = (bytes.len() - 44) as u32;
            bytes[4..8].copy_from_slice(&riff_length.to_le_bytes());
            bytes[40..44].copy_from_slice(&data_length.to_le_bytes());
            fs::write(&original, &bytes).unwrap();
        }
        let metadata = fs::metadata(&original).unwrap();
        let services = support::services(directory.path(), &music).await;
        let app = hito_server::router(services.clone());
        let account = Account::setup(&app).await;
        let (status, _) = account
            .request(
                &app,
                "PUT",
                "/api/v1/metadata/settings",
                json!({"revision":0,"enabled":false,"providers":[]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let libraries = account.get(&app, "/api/v1/libraries").await;
        let library = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["personal"] == false)
            .unwrap();
        let (status, _) = account
            .request(
                &app,
                "POST",
                &format!("/api/v1/libraries/{}/scan", library["id"].as_str().unwrap()),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(services.storage.run_next_job().await.unwrap());
        let recordings = account.get(&app, "/api/v1/recordings").await;
        assert_eq!(recordings["total"], 1);
        assert_eq!(recordings["items"][0]["available"], true);
        let recording = recordings["items"][0]["id"].as_str().unwrap().into();
        Self {
            app,
            services,
            account,
            recording,
            original,
            bytes,
            metadata,
            _directory: directory,
        }
    }
    pub async fn snapshot(&self) -> Value {
        let base = format!("/api/v1/recordings/{}", self.recording);
        json!({
            "catalog": self.account.get(&self.app, "/api/v1/recordings").await,
            "metadata": self.account.get(&self.app, &format!("{base}/metadata")).await,
            "sources": self.account.get(&self.app, &format!("{base}/sources")).await,
            "libraries": self.account.get(&self.app, "/api/v1/libraries").await,
            "artists": self.account.get(&self.app, "/api/v1/artists").await,
            "releases": self.account.get(&self.app, "/api/v1/releases").await,
            "activity": self.account.get(&self.app, "/api/v1/activity").await,
            "plugins": self.account.get(&self.app, "/api/v1/plugins").await,
            "events": self.account.get(&self.app, "/api/v1/sync?cursor=0").await,
        })
    }
    pub fn assert_original(&self) {
        assert_eq!(fs::read(&self.original).unwrap(), self.bytes);
        let current = fs::metadata(&self.original).unwrap();
        assert_eq!(current.len(), self.metadata.len());
        assert_eq!(
            current.modified().unwrap(),
            self.metadata.modified().unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(current.ino(), self.metadata.ino());
        }
    }
}

impl Library {
    pub fn hide_original(&self) {
        fs::rename(
            &self.original,
            self._directory.path().join("offline-original.wav"),
        )
        .unwrap();
    }

    pub fn assert_hidden_original(&self) {
        let hidden = self._directory.path().join("offline-original.wav");
        assert_eq!(fs::read(&hidden).unwrap(), self.bytes);
        let current = fs::metadata(hidden).unwrap();
        assert_eq!(current.len(), self.metadata.len());
        assert_eq!(
            current.modified().unwrap(),
            self.metadata.modified().unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(current.ino(), self.metadata.ino());
        }
    }

    pub fn restore_original(&self) {
        fs::rename(
            self._directory.path().join("offline-original.wav"),
            &self.original,
        )
        .unwrap();
        self.assert_original();
    }
}
