use super::{Setup, install};
use crate::support::{admin, call, write_wav};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    CoreConfig, HostDeviceId, Services,
    test_support::{
        database_owner::DatabaseOwner, hang_guard::hang_guard,
        source_link_service::SourceLinkServer,
    },
};
use hito_plugin_sdk::Capability;
use hito_testkit::fake_plugin::Gate;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::task::JoinSet;

pub(super) type Response = (StatusCode, HeaderMap, Value);

pub(super) struct Owned {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    servers: Vec<Arc<SourceLinkServer>>,
    original_path: PathBuf,
    original: Vec<u8>,
    pub(super) setup: Option<Setup>,
    pub(super) gates: Vec<Arc<Gate>>,
    pub(super) requests: JoinSet<Response>,
}

impl Owned {
    fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("Owned.wav");
        let original = write_wav(&original_path);
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("source-link-http").unwrap(),
        );
        database.attach(&mut config);
        Self {
            _directory: directory,
            database,
            config: Some(config),
            services: None,
            servers: Vec::new(),
            original_path,
            original,
            setup: None,
            gates: Vec::new(),
            requests: JoinSet::new(),
        }
    }

    async fn prepare(&mut self) {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        let seed = self
            .server_with_capabilities("org.hito.source-link-seed", [Capability::Search])
            .await;
        let hito = self.services.as_ref().unwrap();
        let app = hito_server::router(hito.clone());
        let token = admin(&app).await;
        let (status, libraries) =
            call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            &app,
            "POST",
            &format!(
                "/api/v1/libraries/{}/folders",
                libraries[0]["id"].as_str().unwrap()
            ),
            json!({"directory": {"directory_id": "legacy-music", "components": []}}),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(hito.storage.run_next_job().await.unwrap());
        let (status, tracks) =
            call(&app, "GET", "/api/v1/recordings", Value::Null, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let shared = tracks["items"][0]["id"].as_str().unwrap().to_owned();
        let installation = install(hito, &token, &seed).await;
        let (status, adopted) = call(
            &app,
            "POST",
            &format!("/api/v1/plugins/{}/recordings", installation.id),
            json!({
                "installation_revision": installation.revision,
                "provider_recording_id": "owned-personal-recording"
            }),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{adopted}");
        let recording = adopted["id"].as_str().unwrap().to_owned();
        self.setup = Some(Setup {
            hito: hito.clone(),
            app,
            token,
            recording,
            shared,
        });
    }

    pub(super) async fn server(&mut self) -> Arc<SourceLinkServer> {
        let server = hang_guard("owned source-link HTTP provider", SourceLinkServer::start()).await;
        let server = Arc::new(server);
        self.servers.push(server.clone());
        server
    }

    pub(super) async fn server_with_capabilities(
        &mut self,
        id: &str,
        capabilities: impl IntoIterator<Item = Capability>,
    ) -> Arc<SourceLinkServer> {
        let server = hang_guard(
            "owned source-link HTTP capabilities provider",
            SourceLinkServer::with_capabilities(id, capabilities),
        )
        .await;
        let server = Arc::new(server);
        self.servers.push(server.clone());
        server
    }

    async fn retire(&mut self) {
        for gate in &self.gates {
            gate.open();
        }
        self.requests.abort_all();
        let drained = tokio::time::timeout(Duration::from_secs(3), async {
            let mut joined = true;
            while let Some(result) = self.requests.join_next().await {
                joined &= result.is_ok() || result.is_err_and(|error| error.is_cancelled());
            }
            joined
        })
        .await;
        drop(self.setup.take());
        let mut providers_joined = true;
        for server in self.servers.drain(..) {
            match Arc::try_unwrap(server) {
                Ok(mut server) => providers_joined &= server.finish().await,
                Err(_) => providers_joined = false,
            }
        }
        let closed = if let Some(hito) = self.services.take() {
            let playback = hito.playback.close().await;
            let replicas = hito.replicas.close().await;
            let replication = hito.catalog_replication.close().await;
            drop(hito);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
        let retired = self.database.retire(Duration::from_secs(5)).await;
        assert!(matches!(drained, Ok(true)));
        assert!(providers_joined);
        assert!(closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
        assert_eq!(std::fs::read(&self.original_path).unwrap(), self.original);
    }
}

pub(super) async fn run<T>(exercise: impl AsyncFnOnce(&mut Owned) -> T) -> T {
    let mut owned = Owned::new();
    let result = AssertUnwindSafe(Box::pin(async {
        hang_guard("owned source-link HTTP setup", Box::pin(owned.prepare())).await;
        tokio::time::timeout(Duration::from_secs(30), exercise(&mut owned))
            .await
            .expect("The source-link HTTP workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    owned.retire().await;
    match result {
        Ok(value) => value,
        Err(panic) => resume_unwind(panic),
    }
}
