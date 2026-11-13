use crate::{account, credit_member_review_credentials_provider as provider};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    CoreConfig, HostDeviceId, Services,
    test_support::{database_owner::DatabaseOwner, playlist_service::ProviderServer},
};
use hito_testkit::fake_plugin::{FakePlugin, Gate};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;

pub type Response = (StatusCode, HeaderMap, Value);

struct PendingSearch {
    gate: Arc<Gate>,
    task: JoinHandle<Response>,
    entered: bool,
}

pub struct Library {
    directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    services: Option<Services>,
    app: Option<Router>,
    origin: Option<ProviderServer>,
    plugin: Option<FakePlugin>,
    pending: Option<PendingSearch>,
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
            origin: None,
            plugin: None,
            pending: None,
        }
    }

    pub async fn open(&mut self) {
        let music = self.directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let mut config = CoreConfig::new(
            self.directory.path().join("hito.db"),
            music,
            HostDeviceId::new("member-search-credentials").unwrap(),
        );
        self.database.attach(&mut config);
        self.services = Some(hito_core::open(config).await.unwrap());
        self.app = Some(hito_server::router(self.services.as_ref().unwrap().clone()));
        self.origin = Some(ProviderServer::start().await);
        self.plugin = Some(provider::start().await);
    }

    pub fn app(&self) -> &Router {
        self.app.as_ref().unwrap()
    }

    pub fn origin(&self) -> &ProviderServer {
        self.origin.as_ref().unwrap()
    }

    pub fn plugin(&self) -> &FakePlugin {
        self.plugin.as_ref().unwrap()
    }

    pub fn hold_search(&mut self, headers: HeaderMap, path: String, body: Value) {
        assert!(self.pending.is_none());
        let gate = self.plugin().hold_next(provider::SEARCH);
        let app = self.app().clone();
        let task =
            tokio::spawn(
                async move { account::exchange(&app, "POST", &path, body, headers).await },
            );
        self.pending = Some(PendingSearch {
            gate,
            task,
            entered: false,
        });
    }

    pub async fn search_entered(&mut self) -> bool {
        let pending = self.pending.as_mut().unwrap();
        pending.entered = pending.gate.entered_within(Duration::from_secs(2)).await;
        pending.entered
    }

    pub fn search_pending(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| !pending.task.is_finished())
    }

    pub async fn complete_search(&mut self) -> Response {
        let pending = self.pending.as_mut().unwrap();
        pending.gate.release();
        let response = (&mut pending.task).await;
        let retired = !pending.entered || pending.gate.has_retired();
        self.pending.take();
        assert!(retired, "The owned held provider request must retire");
        response.expect("The owned held search must join")
    }

    pub async fn close(mut self) {
        let (joined, held) = if let Some(mut pending) = self.pending.take() {
            pending.gate.release();
            pending.task.abort();
            let joined = match tokio::time::timeout(Duration::from_secs(5), &mut pending.task).await
            {
                Ok(Ok(_)) => true,
                Ok(Err(error)) => error.is_cancelled(),
                Err(_) => false,
            };
            (joined, Some((pending.gate, pending.entered)))
        } else {
            (true, None)
        };
        let provider_joined = match self.plugin.as_mut() {
            Some(plugin) => plugin.finish().await,
            None => true,
        };
        if let Some(origin) = self.origin.take() {
            origin.shutdown().await;
        }
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
        let retired = self.database.retire(Duration::from_secs(5)).await;
        assert!(joined && provider_joined);
        assert!(held.is_none_or(|(gate, entered)| !entered || gate.has_retired()));
        assert!(closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
    }
}
