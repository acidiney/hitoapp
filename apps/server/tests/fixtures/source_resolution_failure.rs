use crate::{
    account::{Account, exchange},
    support,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    Action, CoreConfig, HostDeviceId, Operation, Services,
    test_support::{database_owner::DatabaseOwner, playlist_service},
};
use hito_plugin_sdk::{
    DETAIL_PATH, HEALTH_PATH, HealthReport, MANIFEST_PATH, METADATA_PATH, Manifest,
    ProviderRecording, RecordingMetadata, RecordingRequest, SEARCH_PATH, SOURCES_PATH, SearchPage,
    SearchRequest, SourcePage,
};
use hito_testkit::fake_plugin::{FakePlugin, RecordedRequest, Reply};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) const CANDIDATE: &str = "owned-http-disposition-candidate";
pub(crate) const SOURCE: &str = "owned-http-disposition-audio";
pub(crate) const REFUSAL: &str = "Owned explicit HTTP disposition refusal";
pub(crate) const PRIVATE_TITLE: &str = "Private HTTP disposition title";
pub(crate) const PROVIDER: &str = "org.hito.http-disposition-fixture";
pub(crate) type Response = (StatusCode, HeaderMap, Value);

#[derive(Clone, Copy)]
pub(crate) enum Case {
    Unavailable,
    Refused,
}

impl Case {
    pub(crate) fn failure(self) -> Value {
        match self {
            Self::Unavailable => json!({"reason":"provider_unavailable","retryable":true}),
            Self::Refused => json!({"reason":"provider_refused","retryable":false}),
        }
    }
}

pub(crate) struct Library {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    pub(crate) primary: Option<playlist_service::ProviderServer>,
    pub(crate) provider: Option<FakePlugin>,
    original_path: PathBuf,
    original: Vec<u8>,
}

impl Library {
    pub(crate) fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("owned-http-disposition-original.wav");
        let original = support::write_wav(&original_path);
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("http-disposition-fixture").unwrap(),
        );
        database.attach(&mut config);
        Self {
            _directory: directory,
            database,
            config: Some(config),
            services: None,
            primary: None,
            provider: None,
            original_path,
            original,
        }
    }

    pub(crate) async fn prepare(&mut self, case: Case) -> Context {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        self.primary = Some(playlist_service::ProviderServer::start().await);
        self.provider = Some(provider(case).await);
        let app = hito_server::router(self.services.as_ref().unwrap().clone());
        let owner = Account::setup(&app).await;
        let libraries = owner.get(&app, "/api/v1/libraries").await;
        let jobs = expect(
            &app,
            &owner,
            "POST",
            &format!(
                "/api/v1/libraries/{}/scan",
                libraries[0]["id"].as_str().unwrap()
            ),
            Value::Null,
            StatusCode::ACCEPTED,
        )
        .await;
        assert!(
            self.services
                .as_ref()
                .unwrap()
                .storage
                .run_next_job()
                .await
                .unwrap()
        );
        let job = owner
            .get(
                &app,
                &format!("/api/v1/jobs/{}", jobs[0]["id"].as_str().unwrap()),
            )
            .await;
        assert_eq!(job["state"], "complete");
        let primary = self.primary.as_ref().unwrap();
        primary
            .reply(
                MANIFEST_PATH,
                json!({"id":"org.hito.http-disposition-primary",
            "name":"Owned HTTP disposition primary", "protocol_version":1,
            "capabilities":["playlists"]}),
            )
            .await;
        let primary_installation = owner
            .connect(&app, &primary.url, "Owned HTTP disposition primary", None)
            .await;
        let target = json!({"id":"owned-http-disposition-primary-recording", "title":"Owned tune",
            "artist":"Owned performer", "duration_ms":1000, "revision":"owned-primary-r1",
            "release":{"id":"owned-primary-album","title":"Owned album","artist":"Owned performer"}});
        playlist_service::playlist(
            primary,
            "owned-http-disposition-playlist",
            "owned-http-snapshot",
            vec![
                json!({"occurrence_id":"first", "item":{"kind":"recording","recording":target}}),
                json!({"occurrence_id":"second", "item":{"kind":"recording","recording":target}}),
            ],
        )
        .await;
        let imported = accepted(
            &app,
            &owner,
            "POST",
            &format!(
                "/api/v1/plugins/{}/playlists/import",
                primary_installation["id"].as_str().unwrap()
            ),
            json!({"request_id":Uuid::new_v4(),
                "installation_revision":primary_installation["revision"],
                "playlist_reference":"https://provider.invalid/owned-http-disposition-playlist"}),
        )
        .await;
        let playlist = imported["playlist_id"].as_str().unwrap().to_owned();
        let entries = owner
            .get(&app, &format!("/api/v1/playlists/{playlist}"))
            .await;
        let entries = entries["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["recording_id"], entries[1]["recording_id"]);
        assert_ne!(entries[0]["entry_id"], entries[1]["entry_id"]);
        assert!(entries.iter().all(|entry| entry["available"] == false));
        let recording = entries[0]["recording_id"].as_str().unwrap().to_owned();
        let id = Uuid::parse_str(&recording).unwrap();
        let operations = [
            Action::LikeSet {
                recording_id: id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: id,
                position_ms: 400,
                speed: 1.25,
            },
            Action::RecordingTitleSet {
                recording_id: id,
                title: PRIVATE_TITLE.into(),
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
            &owner,
            "POST",
            "/api/v1/sync",
            json!({"device_id":"http-disposition-sync", "operations":operations}),
        )
        .await;
        assert_eq!(
            synced["acknowledged"],
            json!(operations.iter().map(|op| op.id).collect::<Vec<_>>())
        );
        let installation = owner
            .connect(
                &app,
                &self.provider.as_ref().unwrap().url,
                "Owned HTTP disposition provider",
                None,
            )
            .await;
        let settings = owner.get(&app, "/api/v1/plugins").await;
        let eligible = settings["installations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| {
                let capabilities = item["manifest"]["capabilities"].as_array().unwrap();
                capabilities.contains(&json!("search")) && capabilities.contains(&json!("sources"))
            })
            .collect::<Vec<_>>();
        assert!(eligible.len() == 1 && eligible[0]["id"] == installation["id"]);
        expect(&app, &owner, "POST", "/api/v1/users",
            json!({"username":"http-disposition-peer","password":"owned HTTP disposition peer password"}),
            StatusCode::CREATED).await;
        let peer = Account::login(
            &app,
            "http-disposition-peer",
            "owned HTTP disposition peer password",
        )
        .await;
        Context {
            app,
            owner,
            peer,
            recording,
            playlist,
        }
    }

    pub(crate) async fn primary_counts(&self) -> Vec<usize> {
        let mut counts = Vec::new();
        for path in [
            MANIFEST_PATH,
            "v1/playlists/detail",
            "v1/playlists/items",
            SEARCH_PATH,
            DETAIL_PATH,
            SOURCES_PATH,
            METADATA_PATH,
            "v1/audio",
        ] {
            counts.push(self.primary.as_ref().unwrap().requests(path).await.len());
        }
        counts
    }

    pub(crate) async fn retire(&mut self) {
        let joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        if let Some(primary) = self.primary.take() {
            primary.shutdown().await;
        }
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
        assert!(joined);
        assert!(closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
        assert_eq!(std::fs::read(&self.original_path).unwrap(), self.original);
    }
}

pub(crate) struct Context {
    pub(crate) app: Router,
    pub(crate) owner: Account,
    pub(crate) peer: Account,
    pub(crate) recording: String,
    playlist: String,
}

impl Context {
    pub(crate) fn path(&self) -> String {
        format!("/api/v1/recordings/{}/sources/resolve", self.recording)
    }
    pub(crate) async fn resolve(&self, request: &Value) -> Response {
        exchange(
            &self.app,
            "POST",
            &self.path(),
            request.clone(),
            self.owner.headers(),
        )
        .await
    }
    pub(crate) async fn snapshot(&self) -> Value {
        let metadata = self
            .owner
            .get(
                &self.app,
                &format!("/api/v1/recordings/{}/metadata", self.recording),
            )
            .await;
        let progress = accepted(
            &self.app,
            &self.owner,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids":[self.recording]}),
        )
        .await;
        json!({"metadata":metadata,
            "recordings":stable(self.owner.get(&self.app, "/api/v1/recordings").await, "items"),
            "playlist":stable(self.owner.get(&self.app, &format!("/api/v1/playlists/{}", self.playlist)).await, "entries"),
            "activity":stable(self.owner.get(&self.app, "/api/v1/activity").await, "history"),
            "progress":progress,
            "plugins":self.owner.get(&self.app, "/api/v1/plugins").await})
    }
    pub(crate) async fn peer_snapshot(&self) -> Value {
        json!({"recordings":self.peer.get(&self.app, "/api/v1/recordings").await,
            "playlists":self.peer.get(&self.app, "/api/v1/playlists").await,
            "activity":self.peer.get(&self.app, "/api/v1/activity").await,
            "plugins":self.peer.get(&self.app, "/api/v1/plugins").await,
            "events":self.peer.get(&self.app, "/api/v1/sync?cursor=0").await})
    }
}

fn stable(mut page: Value, field: &str) -> Value {
    for item in page[field].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("available");
        item.as_object_mut().unwrap().remove("plays_from_services");
    }
    page
}

pub(crate) async fn accepted(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
) -> Value {
    expect(app, account, method, path, body, StatusCode::OK).await
}

async fn expect(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
    expected: StatusCode,
) -> Value {
    let (status, headers, body) = exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, expected);
    assert_eq!(headers["cache-control"], "private, no-store");
    body
}

pub(crate) fn valid_search(request: &RecordedRequest) -> bool {
    serde_json::from_slice::<SearchRequest>(&request.body).is_ok_and(|request| {
        request.validate().is_ok()
            && request.query == "Owned performer Owned tune"
            && request.cursor.is_none()
            && request.limit == 20
    })
}

fn valid_recording(request: &RecordedRequest) -> bool {
    serde_json::from_slice::<RecordingRequest>(&request.body)
        .is_ok_and(|request| request.validate().is_ok() && request.recording_id == CANDIDATE)
}

fn detail() -> Value {
    json!({"id":CANDIDATE,"title":"Owned tune","artist":"Owned performer","duration_ms":1000,
        "release":null,"revision":"owned-http-disposition-r1"})
}

pub(crate) fn ready_search() -> Value {
    let page = json!({"items":[detail()],"next_cursor":null});
    serde_json::from_value::<SearchPage>(page.clone())
        .unwrap()
        .validate(20)
        .unwrap();
    page
}

async fn provider(case: Case) -> FakePlugin {
    let manifest: Manifest = serde_json::from_value(json!({"id":PROVIDER,
        "name":"Owned HTTP disposition provider","protocol_version":1,
        "capabilities":["search","sources","metadata"]}))
    .unwrap();
    let detail = detail();
    let sources =
        json!({"items":[{"id":SOURCE,"label":"Owned HTTP disposition audio","ready":true}]});
    let metadata = json!({"recording":detail,"musical_title":"Owned tune", "album_title":"Owned album",
        "source_ids":[SOURCE]});
    serde_json::from_value::<ProviderRecording>(detail.clone())
        .unwrap()
        .validate()
        .unwrap();
    serde_json::from_value::<SourcePage>(sources.clone())
        .unwrap()
        .validate()
        .unwrap();
    serde_json::from_value::<RecordingMetadata>(metadata.clone())
        .unwrap()
        .validate()
        .unwrap();
    FakePlugin::builder(manifest)
        .require_protocol()
        .require_json()
        .strict(16)
        .get(HEALTH_PATH, |_| {
            Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
        })
        .post(SEARCH_PATH, move |request| {
            if !valid_search(request) {
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            match case {
                Case::Unavailable => Reply::Status(StatusCode::SERVICE_UNAVAILABLE),
                Case::Refused => Reply::Raw {
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                    headers: HeaderMap::from_iter([(
                        "content-type".parse().unwrap(),
                        "application/json".parse().unwrap(),
                    )]),
                    body: serde_json::to_vec(&json!({"error":REFUSAL})).unwrap(),
                    chunked: false,
                },
            }
        })
        .post(DETAIL_PATH, move |request| {
            if !valid_recording(request) {
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            Reply::Json(detail.clone())
        })
        .post(SOURCES_PATH, move |request| {
            if !valid_recording(request) {
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            Reply::Json(sources.clone())
        })
        .post(METADATA_PATH, move |request| {
            if !valid_recording(request) {
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            Reply::Json(metadata.clone())
        })
        .start()
        .await
}
