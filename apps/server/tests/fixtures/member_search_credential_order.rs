use crate::{
    account::{Account, exchange},
    artist_review_fixture::origin_counts,
    catalog_album_header_fixture as catalog,
    member_search_credential_provider::{self as provider, SearchOutcome},
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
    test_support::database_owner::DatabaseOwner,
};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) type Response = (StatusCode, HeaderMap, Value);

pub(crate) struct Library {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    pub(crate) origin: Option<ProviderServer>,
    pub(crate) provider: Option<FakePlugin>,
    original_path: PathBuf,
    original: Vec<u8>,
}

impl Library {
    pub(crate) fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("owned-member-order-original.wav");
        support::write_wav(&original_path);
        let original = std::fs::read(&original_path).unwrap();
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("member-search-order-http").unwrap(),
        );
        database.attach(&mut config);
        Self {
            _directory: directory,
            database,
            config: Some(config),
            services: None,
            origin: None,
            provider: None,
            original_path,
            original,
        }
    }

    pub(crate) async fn prepare(&mut self, outcome: SearchOutcome) -> Context {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        self.origin = Some(ProviderServer::start().await);
        self.provider = Some(provider::start(outcome).await);
        let app = hito_server::router(self.services.as_ref().unwrap().clone());
        let owner = Account::setup(&app).await;
        let imported = Imported::create(&app, &owner, self.origin.as_ref().unwrap()).await;
        let plugin = self.provider.as_ref().unwrap();
        let installation = owner
            .connect(
                &app,
                &plugin.url,
                "Owned member ordering service",
                Some(provider::TOKEN),
            )
            .await;
        plugin.require_bearer(provider::TOKEN);
        let selected = MetadataProvider::HttpPlugin {
            installation_id: installation["id"].as_str().unwrap().into(),
        };
        let settings = owner.get(&app, "/api/v1/metadata/settings").await;
        accepted(
            &app,
            &owner,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
        let review_path = format!("/api/v1/recordings/{}/artist-review", imported.recording);
        let whole = accepted(
            &app,
            &owner,
            "POST",
            &format!("{review_path}/context"),
            json!({"provider": selected}),
        )
        .await;
        let association = accepted(
            &app,
            &owner,
            "POST",
            &format!("{review_path}/confirm"),
            json!({"context": whole, "provider_artist_id": catalog::ARTIST}),
        )
        .await;
        assert_eq!(
            association["provider"]["installation_id"],
            installation["id"]
        );
        assert_eq!(association["artist_id"], whole["artist_id"]);
        accepted(
            &app,
            &owner,
            "PUT",
            &format!("/api/v1/recordings/{}/artist-identity", imported.recording),
            json!({"request_id": Uuid::new_v4(), "expected_revision": 0,
                "association_id": association["id"], "context": whole}),
        )
        .await;
        let member_path = format!(
            "/api/v1/recordings/{}/credit-members/{}/artist-review",
            imported.recording, imported.member,
        );
        let context = accepted(
            &app,
            &owner,
            "POST",
            &format!("{member_path}/context"),
            json!({"provider": selected}),
        )
        .await;
        assert_eq!(context["account_id"], owner.id);
        assert_eq!(context["recording_id"], imported.recording);
        assert_eq!(context["artist_id"], imported.member);
        assert_ne!(context["artist_id"], whole["artist_id"]);
        assert_eq!(context["role"], "featured");
        assert_eq!(context["connection"], whole["connection"]);
        let (status, _) = owner.request(&app, "POST", "/api/v1/users", json!({
            "username": "member-order-peer", "password": "owned member ordering peer password",
        })).await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = Account::login(
            &app,
            "member-order-peer",
            "owned member ordering peer password",
        )
        .await;
        let baseline = owner.get(&app, "/api/v1/plugins").await;
        let current = installation_by_id(&baseline, &installation["id"]).clone();
        assert_eq!(current, installation);
        assert!(current.get("health").is_none());
        Context {
            app,
            owner,
            peer,
            imported,
            association,
            context,
            installation: current,
            member_path,
            selected,
        }
    }

    pub(crate) async fn retire(&mut self) {
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
        assert!(joined, "The owned ordering provider must join");
        assert!(closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
        assert_eq!(std::fs::read(&self.original_path).unwrap(), self.original);
    }

    pub(crate) async fn origin_counts(&self) -> Vec<usize> {
        origin_counts(self.origin.as_ref().unwrap()).await
    }
}

pub(crate) struct Context {
    pub(crate) app: Router,
    pub(crate) owner: Account,
    pub(crate) peer: Account,
    pub(crate) imported: Imported,
    pub(crate) association: Value,
    pub(crate) context: Value,
    pub(crate) installation: Value,
    pub(crate) member_path: String,
    selected: MetadataProvider,
}

impl Context {
    pub(crate) fn search_path(&self) -> String {
        format!("{}/search", self.member_path)
    }

    pub(crate) fn search_body(&self) -> Value {
        json!({"context": self.context, "search": provider::request()})
    }

    pub(crate) async fn group(&self) -> Response {
        exchange(
            &self.app,
            "POST",
            &format!(
                "/api/v1/recordings/{}/artist-associations/{}/release-group",
                self.imported.recording,
                self.association["id"].as_str().unwrap(),
            ),
            json!({"release_group_id": catalog::GROUP}),
            self.owner.headers(),
        )
        .await
    }

    pub(crate) async fn current_installation(&self) -> Value {
        let plugins = self.owner.get(&self.app, "/api/v1/plugins").await;
        installation_by_id(&plugins, &self.installation["id"]).clone()
    }

    pub(crate) async fn snapshot(&self) -> Value {
        let mut state = self.imported.snapshot(&self.app, &self.owner).await;
        let installation = state["plugins"]["installations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|item| item["id"] == self.installation["id"])
            .unwrap();
        installation.as_object_mut().unwrap().remove("health");
        json!({
            "library": state,
            "choice": self.owner.get(&self.app,
                &format!("/api/v1/recordings/{}/artist-identity", self.imported.recording)).await,
            "associations": self.owner.get(&self.app,
                &format!("/api/v1/recordings/{}/artist-associations", self.imported.recording)).await,
        })
    }

    pub(crate) async fn peer_snapshot(&self) -> Value {
        json!({
            "recordings": self.peer.get(&self.app, "/api/v1/recordings").await,
            "playlists": self.peer.get(&self.app, "/api/v1/playlists").await,
            "activity": self.peer.get(&self.app, "/api/v1/activity").await,
            "plugins": self.peer.get(&self.app, "/api/v1/plugins").await,
            "settings": self.peer.get(&self.app, "/api/v1/metadata/settings").await,
            "groups": self.peer.get(&self.app, "/api/v1/artist-identity-groups").await,
        })
    }

    pub(crate) async fn assert_current_context(&self) {
        let current = accepted(
            &self.app,
            &self.owner,
            "POST",
            &format!("{}/context", self.member_path),
            json!({"provider": self.selected}),
        )
        .await;
        assert_eq!(current, self.context);
    }
}

pub(crate) fn installation_by_id<'a>(settings: &'a Value, id: &Value) -> &'a Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == *id)
        .unwrap()
}

pub(crate) fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}

pub(crate) fn rejected(response: &Response, installation: &Value) {
    assert_eq!(response.0, StatusCode::CONFLICT);
    private(&response.1);
    assert_eq!(response.2["code"], "plugin_credentials_required");
    assert_eq!(
        response.2["plugin_connection"],
        json!({
            "installation_id": installation["id"], "installation_revision": installation["revision"],
        })
    );
    assert_eq!(response.2.as_object().unwrap().len(), 3);
    assert!(!response.2.to_string().contains(provider::TOKEN));
}

pub(crate) async fn accepted(
    app: &Router,
    owner: &Account,
    method: &str,
    path: &str,
    body: Value,
) -> Value {
    let (status, headers, body) = exchange(app, method, path, body, owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    body
}
