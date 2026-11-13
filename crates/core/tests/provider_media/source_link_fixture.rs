use super::connect;
use futures_util::FutureExt;
use hito_core::{
    Actor, AdoptProviderRecording, CoreConfig, Credentials, Error, HostDeviceId,
    ProviderCandidate, Services,
    test_support::{
        database_owner::DatabaseOwner, hang_guard::hang_guard,
        source_link_service::SourceLinkServer, support,
    },
};
use hito_plugin_sdk::Capability;
use hito_testkit::fake_plugin::Gate;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::task::JoinSet;

pub(super) struct World {
    pub(super) hito: Services,
    pub(super) actor: Actor,
    pub(super) recording: String,
    pub(super) shared: String,
}

pub(super) struct Owned {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    servers: Vec<Arc<SourceLinkServer>>,
    original_path: PathBuf,
    original: Vec<u8>,
    pub(super) world: Option<World>,
    pub(super) gates: Vec<Arc<Gate>>,
    pub(super) requests: JoinSet<Result<ProviderCandidate, Error>>,
}

impl Owned {
    fn new(name: &str) -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original_path = music.join("owned.wav");
        let original = support::write_wav(&original_path, 11);
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new(name).unwrap(),
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
            world: None,
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
        let session = hito
            .identity
            .setup(Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let library = hito.catalog.libraries(&actor).await.unwrap().remove(0).id;
        support::scan(hito, &actor, &library).await;
        let shared = hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let installation = connect(hito, &actor, &seed).await;
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: installation.revision,
                    provider_recording_id: "owned-personal-recording".into(),
                },
            )
            .await
            .unwrap()
            .id;
        self.world = Some(World {
            hito: hito.clone(),
            actor,
            recording,
            shared,
        });
    }

    pub(super) async fn server(&mut self) -> Arc<SourceLinkServer> {
        let server = hang_guard("owned source-link Core provider", SourceLinkServer::start()).await;
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
            "owned source-link Core capabilities provider",
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
        drop(self.world.take());
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

pub(super) async fn run(name: &str, exercise: impl AsyncFnOnce(&mut Owned)) {
    let mut owned = Owned::new(name);
    let result = AssertUnwindSafe(Box::pin(async {
        hang_guard("owned source-link Core setup", Box::pin(owned.prepare())).await;
        tokio::time::timeout(Duration::from_secs(30), exercise(&mut owned))
            .await
            .expect("The source-link Core workflow must finish within its deadline");
    }))
    .catch_unwind()
    .await;
    owned.retire().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}
