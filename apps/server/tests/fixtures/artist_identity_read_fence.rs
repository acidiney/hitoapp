use crate::{
    account::{Account, exchange},
    artist_review_fixture::origin_counts,
    credit_member_review_preview_fixture::{accepted, private},
    member_search_order_library::Imported,
    provider_fixture::ProviderServer,
    support,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    CoreConfig, HostDeviceId, MetadataProvider, Services,
    test_support::{artist_catalog_discovery_service as catalog, database_owner::DatabaseOwner},
};
use hito_testkit::held_authority_snapshot::HeldSnapshot;
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

pub(crate) type Response = (StatusCode, HeaderMap, Value);

pub(crate) struct Ready {
    pub(crate) app: Router,
    pub(crate) owner: Account,
    pub(crate) inspector: Account,
    pub(crate) shared: String,
    pub(crate) local: String,
    pub(crate) imported: String,
    pub(crate) member: String,
    pub(crate) group: String,
    selected: MetadataProvider,
}

impl Ready {
    pub(crate) async fn snapshot(&self) -> Value {
        let mut targets = Vec::new();
        for recording in [&self.local, &self.imported] {
            targets.push(json!({
                "metadata": self.inspector.get(&self.app, &format!("/api/v1/recordings/{recording}/metadata")).await,
                "sources": self.inspector.get(&self.app, &format!("/api/v1/recordings/{recording}/sources")).await,
                "choice": self.inspector.get(&self.app, &format!("/api/v1/recordings/{recording}/artist-identity")).await,
                "associations": self.inspector.get(&self.app, &format!("/api/v1/recordings/{recording}/artist-associations")).await,
            }));
        }
        let playlists = self.inspector.get(&self.app, "/api/v1/playlists").await;
        let mut copies = Vec::new();
        for playlist in playlists.as_array().unwrap() {
            copies.push(
                self.inspector
                    .get(
                        &self.app,
                        &format!("/api/v1/playlists/{}", playlist["id"].as_str().unwrap()),
                    )
                    .await,
            );
        }
        json!({
            "targets": targets, "playlists": playlists, "copies": copies,
            "recordings": self.inspector.get(&self.app, "/api/v1/recordings").await,
            "libraries": self.inspector.get(&self.app, "/api/v1/libraries").await,
            "artists": self.inspector.get(&self.app, "/api/v1/artists").await,
            "groups": self.inspector.get(&self.app, "/api/v1/artist-identity-groups").await,
            "detail": self.inspector.get(&self.app, &format!("/api/v1/artist-identity-groups/{}", self.group)).await,
            "events": self.inspector.get(&self.app, "/api/v1/sync?cursor=0").await,
            "activity": self.inspector.get(&self.app, "/api/v1/activity").await,
            "plugins": self.inspector.get(&self.app, "/api/v1/plugins").await,
            "policy": self.inspector.get(&self.app, "/api/v1/metadata/settings").await,
            "progress": accepted(&self.app, &self.inspector, "POST", "/api/v1/recordings/progress",
                json!({"recording_ids": [self.local, self.imported]})).await,
        })
    }

    pub(crate) async fn choose_member(&self) -> String {
        let prefix = format!(
            "/api/v1/recordings/{}/credit-members/{}",
            self.imported, self.member
        );
        let context = accepted(
            &self.app,
            &self.owner,
            "POST",
            &format!("{prefix}/artist-review/context"),
            json!({"provider": self.selected}),
        )
        .await;
        let association = accepted(
            &self.app,
            &self.owner,
            "POST",
            &format!("{prefix}/artist-review/confirm"),
            json!({"context": context, "provider_artist_id": catalog::SECOND_ARTIST}),
        )
        .await;
        let path = format!("{prefix}/artist-identity");
        let receipt = accepted(
            &self.app,
            &self.owner,
            "PUT",
            &path,
            json!({"request_id": Uuid::new_v4(), "expected_revision": 0,
                "association_id": association["id"], "context": context}),
        )
        .await;
        assert_eq!(receipt["revision"], 1);
        assert_eq!(receipt["selection"]["association_id"], association["id"]);
        let value = self.owner.get(&self.app, &path).await;
        assert_eq!(value["state"]["status"], "selected");
        path
    }
}

pub(crate) struct Fixture {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    origin: Option<ProviderServer>,
    provider: Option<catalog::DiscoveryServer>,
    audio: PathBuf,
    original: Vec<u8>,
    pub(crate) ready: Option<Ready>,
    pub(crate) hold: HeldSnapshot,
    pub(crate) requests: JoinSet<Response>,
}

impl Fixture {
    pub(crate) fn new(point: &'static str) -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let audio = music.join("owned-read-fence.wav");
        support::write_wav(&audio);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Owned read fence local recording".into());
        tag.set_artist("Owned read fence local performer".into());
        tag.save_to_path(&audio, Default::default()).unwrap();
        let original = std::fs::read(&audio).unwrap();
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("member-read-fence").unwrap(),
        );
        database.attach(&mut config);
        Self {
            _directory: directory,
            database,
            config: Some(config),
            services: None,
            origin: None,
            provider: None,
            audio,
            original,
            ready: None,
            hold: HeldSnapshot::at(point),
            requests: JoinSet::new(),
        }
    }

    pub(crate) async fn prepare(&mut self) {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        self.origin = Some(ProviderServer::start().await);
        self.provider = Some(catalog::DiscoveryServer::start().await);
        let services = self.services.as_ref().unwrap();
        let app = hito_server::router(services.clone());
        let owner = Account::setup(&app).await;
        let imported = Imported::create(&app, &owner, self.origin.as_ref().unwrap()).await;
        let libraries = owner.get(&app, "/api/v1/libraries").await;
        let shared = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|library| library["personal"] == false)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            owner
                .request(
                    &app,
                    "POST",
                    &format!("/api/v1/libraries/{shared}/scan"),
                    Value::Null
                )
                .await
                .0,
            StatusCode::ACCEPTED
        );
        services.storage.run_next_job().await.unwrap();
        let page = owner
            .get(&app, &format!("/api/v1/recordings?library={shared}"))
            .await;
        assert_eq!(page["total"], 1);
        let local = page["items"][0]["id"].as_str().unwrap().to_owned();
        let provider = self.provider.as_ref().unwrap();
        let installation = owner
            .connect(
                &app,
                &provider.url,
                "Owned read fence catalog",
                Some(catalog::TOKEN),
            )
            .await;
        let selected = MetadataProvider::HttpPlugin {
            installation_id: installation["id"].as_str().unwrap().into(),
        };
        let policy = owner.get(&app, "/api/v1/metadata/settings").await;
        accepted(
            &app,
            &owner,
            "PUT",
            "/api/v1/metadata/settings",
            json!({"revision": policy["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
        let mut groups = Vec::new();
        for recording in [&imported.recording, &local] {
            let prefix = format!("/api/v1/recordings/{recording}");
            let context = accepted(
                &app,
                &owner,
                "POST",
                &format!("{prefix}/artist-review/context"),
                json!({"provider": selected}),
            )
            .await;
            let association = accepted(
                &app,
                &owner,
                "POST",
                &format!("{prefix}/artist-review/confirm"),
                json!({"context": context, "provider_artist_id": catalog::FIRST_ARTIST}),
            )
            .await;
            let receipt = accepted(
                &app,
                &owner,
                "PUT",
                &format!("{prefix}/artist-identity"),
                json!({"request_id": Uuid::new_v4(), "expected_revision": 0,
                    "association_id": association["id"], "context": context}),
            )
            .await;
            groups.push(
                receipt["selection"]["group_id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        assert_eq!(groups[0], groups[1]);
        let inspector =
            Account::login(&app, "owner", "owned artist review administrator password").await;
        self.ready = Some(Ready {
            app,
            owner,
            inspector,
            shared,
            local,
            imported: imported.recording,
            member: imported.member,
            group: groups.remove(0),
            selected,
        });
    }

    pub(crate) fn begin(&mut self, path: String) {
        let ready = self.ready.as_ref().unwrap();
        let app = ready.app.clone();
        let headers = ready.owner.headers();
        hito_testkit::logs::anchor_callsite_interest();
        let subscriber = tracing_subscriber::registry().with(self.hold.clone());
        self.requests.spawn(
            async move { exchange(&app, "GET", &path, Value::Null, headers).await }
                .with_subscriber(subscriber),
        );
    }

    pub(crate) async fn counts(&self) -> Value {
        json!({"catalog": self.provider.as_ref().unwrap().requests().len(),
            "unexpected": self.provider.as_ref().unwrap().unexpected(),
            "origin": origin_counts(self.origin.as_ref().unwrap()).await})
    }

    pub(crate) async fn retire(&mut self) {
        self.hold.release();
        self.requests.abort_all();
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.requests.join_next().await.is_some() {}
        })
        .await
        .unwrap();
        drop(self.ready.take());
        let joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        if let Some(origin) = self.origin.take() {
            origin.shutdown().await;
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
        assert!(joined && closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
        assert_eq!(std::fs::read(&self.audio).unwrap(), self.original);
    }
}

pub(crate) fn assert_denied(response: &Response, expected: StatusCode) {
    assert_eq!(
        response.0, expected,
        "A protected read may not return data from a snapshot older than committed revocation"
    );
    private(&response.1);
    assert!(response.2.get("items").is_none());
    assert!(response.2.get("group").is_none());
    assert!(response.2.get("state").is_none());
}
