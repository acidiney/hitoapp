use crate::source_authority_fixture as provider;
use crate::source_credential_fixture::{Context, OWNER_TOKEN, ProviderServer, Response, account};
use axum::http::StatusCode;
use hito_plugin_sdk::{PROTOCOL_VERSION, SEARCH_PATH, SearchRequest};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use hito_testkit::held_authority_snapshot::HeldSnapshot;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

pub(crate) struct ReadySource {
    pub(crate) path: String,
    pub(crate) before: Value,
}

pub(crate) struct SnapshotFixture {
    pub(crate) primary: ProviderServer,
    pub(crate) previous: ProviderServer,
    pub(crate) plugin: FakePlugin,
    pub(crate) hold: HeldSnapshot,
    pub(crate) requests: JoinSet<Response>,
    pub(crate) context: Option<Context>,
    pub(crate) inspector: Option<account::Account>,
    pub(crate) ready: Option<ReadySource>,
}

impl SnapshotFixture {
    pub(crate) async fn start(id: &str, point: &'static str) -> Self {
        Self {
            primary: ProviderServer::start().await,
            previous: ProviderServer::start().await,
            plugin: provider::start(id, true, OWNER_TOKEN).await,
            hold: HeldSnapshot::at(point),
            requests: JoinSet::new(),
            context: None,
            inspector: None,
            ready: None,
        }
    }

    pub(crate) fn reject_search(&mut self) {
        self.plugin.on(SEARCH_PATH, |request| {
            assert_eq!(request.method.as_str(), "POST");
            assert_eq!(
                request.protocol().as_deref(),
                Some(PROTOCOL_VERSION.to_string().as_str())
            );
            assert_eq!(
                request.headers["authorization"],
                format!("Bearer {OWNER_TOKEN}").as_str()
            );
            let search: SearchRequest = serde_json::from_value(request.json()).unwrap();
            assert!(search.validate().is_ok());
            assert_eq!(search.query, "Owned performer Owned tune");
            assert!(search.cursor.is_none());
            assert_eq!(search.limit, 20);
            Reply::Status(StatusCode::UNAUTHORIZED)
        });
    }

    pub(crate) async fn begin(&mut self) -> Value {
        let context = self
            .context
            .insert(Context::prepare(&self.primary, &self.previous, &self.plugin).await);
        let inspector =
            account::Account::login(&context.app, "admin", "indexing test password").await;
        assert_eq!(inspector.id, context.owner.id);
        let catalog = inspector.get(&context.app, "/api/v1/recordings").await;
        let ready_recording = catalog["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["available"] == true)
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let path = format!("/api/v1/recordings/{ready_recording}/sources");
        let before = inspector.get(&context.app, &path).await;
        assert!(
            before
                .as_array()
                .unwrap()
                .iter()
                .any(|source| source["ready"] == true)
        );
        self.inspector = Some(inspector);
        self.ready = Some(ReadySource { path, before });
        let request = json!({"request_id":Uuid::new_v4()});
        let app = context.app.clone();
        let path = context.path();
        let headers = context.owner.headers();
        let pending_request = request.clone();
        hito_testkit::logs::anchor_callsite_interest();
        let subscriber = tracing_subscriber::registry().with(self.hold.clone());
        self.requests.spawn(
            async move { account::exchange(&app, "POST", &path, pending_request, headers).await }
                .with_subscriber(subscriber),
        );
        request
    }

    pub(crate) async fn finish(mut self) -> (usize, Option<Context>) {
        self.hold.release();
        self.requests.abort_all();
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.requests.join_next().await.is_some() {}
        })
        .await
        .unwrap();
        let dispatched = self.plugin.requests_for(SEARCH_PATH).len();
        let stopped = self.plugin.finish().await;
        self.primary.shutdown().await;
        self.previous.shutdown().await;
        assert!(stopped);
        (dispatched, self.context)
    }
}
