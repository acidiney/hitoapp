pub(crate) use crate::member_search_credential_order_fixture::{accepted, private};
use crate::{
    account::Account,
    artist_review_fixture::origin_counts,
    member_search_order_library::{Imported, MEMBER},
    provider_fixture::ProviderServer,
    support,
};
use axum::{Router, http::StatusCode};
use hito_core::{
    CoreConfig, CreditMemberReviewContext, HostDeviceId, MetadataProvider, Services,
    test_support::{artist_catalog_discovery_service as catalog, database_owner::DatabaseOwner},
};
use hito_plugin_sdk::CreditRole;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) struct Library {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    origin: Option<ProviderServer>,
    pub(crate) provider: Option<catalog::DiscoveryServer>,
    original_path: PathBuf,
    original: Vec<u8>,
}

impl Library {
    pub(crate) fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("owned-member-preview-original.wav");
        support::write_wav(&original_path);
        let original = std::fs::read(&original_path).unwrap();
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("member-preview-http").unwrap(),
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

    pub(crate) async fn prepare(&mut self) -> Context {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        self.origin = Some(ProviderServer::start().await);
        self.provider = Some(catalog::DiscoveryServer::start().await);
        let app = hito_server::router(self.services.as_ref().unwrap().clone());
        let account = Account::setup(&app).await;
        let imported = Imported::create(&app, &account, self.origin.as_ref().unwrap()).await;
        let provider = self.provider.as_ref().unwrap();
        let installation = account
            .connect(
                &app,
                &provider.url,
                "Owned member preview catalog",
                Some(catalog::TOKEN),
            )
            .await;
        let selected = MetadataProvider::HttpPlugin {
            installation_id: installation["id"].as_str().unwrap().into(),
        };
        let settings = account.get(&app, "/api/v1/metadata/settings").await;
        accepted(
            &app,
            &account,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
        let whole_path = format!("/api/v1/recordings/{}/artist-review", imported.recording);
        let whole = accepted(
            &app,
            &account,
            "POST",
            &format!("{whole_path}/context"),
            json!({"provider": selected}),
        )
        .await;
        let association = accepted(
            &app,
            &account,
            "POST",
            &format!("{whole_path}/confirm"),
            json!({"context": whole, "provider_artist_id": catalog::FIRST_ARTIST}),
        )
        .await;
        let chosen = accepted(
            &app,
            &account,
            "PUT",
            &format!("/api/v1/recordings/{}/artist-identity", imported.recording),
            json!({"request_id": Uuid::new_v4(), "expected_revision": 0,
                "association_id": association["id"], "context": whole}),
        )
        .await;
        assert_eq!(chosen["revision"], 1);
        assert_eq!(chosen["selection"]["association_id"], association["id"]);
        assert_eq!(association["provider_artist_id"], catalog::FIRST_ARTIST);
        let member_path = format!(
            "/api/v1/recordings/{}/credit-members/{}/artist-review",
            imported.recording, imported.member
        );
        let context = accepted(
            &app,
            &account,
            "POST",
            &format!("{member_path}/context"),
            json!({"provider": selected}),
        )
        .await;
        let captured: CreditMemberReviewContext = serde_json::from_value(context.clone()).unwrap();
        assert_eq!(captured.account_id, account.id);
        assert_eq!(captured.recording_id, imported.recording);
        assert_eq!(captured.artist_id, imported.member);
        assert_eq!(captured.credited_name, MEMBER);
        assert!(matches!(captured.role, CreditRole::Featured));
        assert_eq!(context["connection"], whole["connection"]);
        assert_ne!(context["artist_id"], whole["artist_id"]);
        let (status, _) = account.request(&app, "POST", "/api/v1/users", json!({
            "username": "member-preview-peer", "password": "owned member preview peer password",
        })).await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = Account::login(
            &app,
            "member-preview-peer",
            "owned member preview peer password",
        )
        .await;
        Context {
            app,
            account,
            peer,
            imported,
            member_path,
            context,
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
        assert!(joined, "The owned discovery provider must join");
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
    pub(crate) account: Account,
    pub(crate) peer: Account,
    imported: Imported,
    pub(crate) member_path: String,
    pub(crate) context: Value,
    selected: MetadataProvider,
}

impl Context {
    pub(crate) async fn snapshot(&self) -> Value {
        json!({
            "library": self.imported.snapshot(&self.app, &self.account).await,
            "choice": self.account.get(&self.app,
                &format!("/api/v1/recordings/{}/artist-identity", self.imported.recording)).await,
            "associations": self.account.get(&self.app,
                &format!("/api/v1/recordings/{}/artist-associations", self.imported.recording)).await,
        })
    }

    pub(crate) async fn assert_context_preserved(&self) {
        let context = accepted(
            &self.app,
            &self.account,
            "POST",
            &format!("{}/context", self.member_path),
            json!({"provider": self.selected}),
        )
        .await;
        assert_eq!(context, self.context);
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
