use crate::{
    account::{Account, exchange},
    album_editions::{Prepared, prepare},
    support,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    Action, CoreConfig, HostDeviceId, Operation, Services,
    test_support::{album_edition_service::AlbumProvider, database_owner::DatabaseOwner},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) const PRIVATE_TITLE: &str = "Owned private parity title";

pub(crate) struct Library {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    pub(crate) provider: Option<AlbumProvider>,
    original_path: PathBuf,
    original: Vec<u8>,
}

impl Library {
    pub(crate) fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("owned-digest-parity-original.wav");
        support::write_wav(&original_path);
        let original = std::fs::read(&original_path).unwrap();
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("edition-digest-parity-http").unwrap(),
        );
        database.attach(&mut config);
        Self {
            _directory: directory,
            database,
            config: Some(config),
            services: None,
            provider: None,
            original_path,
            original,
        }
    }

    pub(crate) async fn prepare(&mut self) -> Context {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        self.provider = Some(AlbumProvider::start().await);
        let app = hito_server::router(self.services.as_ref().unwrap().clone());
        let account = Account::setup(&app).await;
        let prepared = Box::pin(prepare(&app, &account, self.provider.as_ref().unwrap())).await;
        let playlist = Uuid::new_v4();
        let first = Uuid::parse_str(&prepared.first).unwrap();
        let second = Uuid::parse_str(prepared.saved[1]["id"].as_str().unwrap()).unwrap();
        let operations = [
            Action::RecordingTitleSet {
                recording_id: second,
                title: PRIVATE_TITLE.into(),
            },
            Action::LikeSet {
                recording_id: first,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: first,
                position_ms: 400,
                speed: 1.25,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Private parity selection".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id: first,
                position: 0,
            },
        ]
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect::<Vec<_>>();
        let synced = accepted(
            &app,
            &account,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "owned-edition-digest-parity", "operations": operations,
            }),
        )
        .await;
        assert_eq!(
            synced["acknowledged"],
            json!(operations.iter().map(|op| op.id).collect::<Vec<_>>())
        );
        let (status, _) = account.request(&app, "POST", "/api/v1/users", json!({
            "username": "digest-parity-peer", "password": "owned digest parity peer password",
        })).await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = Account::login(
            &app,
            "digest-parity-peer",
            "owned digest parity peer password",
        )
        .await;
        Context {
            app,
            account,
            peer,
            prepared,
            playlist,
        }
    }

    pub(crate) async fn retire(&mut self) {
        let joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        let closed = if let Some(services) = self.services.take() {
            let playback = services.playback.close().await;
            let replicas = services.replicas.close().await;
            let replication = services.catalog_replication.close().await;
            drop(services);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
        let retired = self.database.retire(Duration::from_secs(5)).await;
        assert!(joined, "The owned parity provider must join");
        assert!(closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
        assert_eq!(std::fs::read(&self.original_path).unwrap(), self.original);
    }
}

pub(crate) struct Context {
    pub(crate) app: Router,
    pub(crate) account: Account,
    pub(crate) peer: Account,
    pub(crate) prepared: Prepared,
    playlist: Uuid,
}

impl Context {
    pub(crate) async fn snapshot(&self) -> Value {
        let mut metadata = Vec::new();
        for saved in &self.prepared.saved {
            let id = saved["id"].as_str().unwrap();
            metadata.push(
                self.account
                    .get(&self.app, &format!("/api/v1/recordings/{id}/metadata"))
                    .await,
            );
            assert_eq!(
                self.account
                    .get(&self.app, &format!("/api/v1/recordings/{id}/sources"))
                    .await,
                json!([])
            );
            let (status, _) = self
                .account
                .request(
                    &self.app,
                    "GET",
                    &format!("/api/v1/recordings/{id}/download"),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
        assert_eq!(metadata[1]["title"]["personal"], PRIVATE_TITLE);
        let progress = accepted(
            &self.app,
            &self.account,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [self.prepared.first]}),
        )
        .await;
        assert_eq!(progress[0]["position_ms"], 400);
        assert_eq!(progress[0]["speed"], 1.25);
        let recordings = self.account.get(&self.app, "/api/v1/recordings").await;
        assert_eq!(recordings["total"], 3);
        assert_eq!(
            recordings["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|recording| recording["id"] == self.prepared.first)
                .unwrap()["liked"],
            true
        );
        let playlist = self
            .account
            .get(&self.app, &format!("/api/v1/playlists/{}", self.playlist))
            .await;
        assert_eq!(playlist["entries"].as_array().unwrap().len(), 1);
        assert_eq!(playlist["entries"][0]["recording_id"], self.prepared.first);
        json!({
            "album": self.account.get(&self.app, &format!("/api/v1/releases/{}", self.prepared.release)).await,
            "recordings": recordings, "metadata": metadata, "progress": progress, "playlist": playlist,
            "releases": self.account.get(&self.app, "/api/v1/releases").await,
            "artists": self.account.get(&self.app, "/api/v1/artists").await,
            "libraries": self.account.get(&self.app, "/api/v1/libraries").await,
            "activity": self.account.get(&self.app, "/api/v1/activity").await,
            "events": self.account.get(&self.app, "/api/v1/sync?cursor=0").await,
            "plugins": self.account.get(&self.app, "/api/v1/plugins").await,
            "policy": self.account.get(&self.app, "/api/v1/metadata/settings").await,
            "associations": self.account.get(&self.app,
                &format!("/api/v1/recordings/{}/artist-associations", self.prepared.first)).await,
        })
    }

    pub(crate) async fn peer_snapshot(&self) -> Value {
        json!({
            "recordings": self.peer.get(&self.app, "/api/v1/recordings").await,
            "playlists": self.peer.get(&self.app, "/api/v1/playlists").await,
            "plugins": self.peer.get(&self.app, "/api/v1/plugins").await,
            "activity": self.peer.get(&self.app, "/api/v1/activity").await,
        })
    }
}

pub(crate) async fn accepted(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
) -> Value {
    let (status, headers, response) = exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    response
}

pub(crate) fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
