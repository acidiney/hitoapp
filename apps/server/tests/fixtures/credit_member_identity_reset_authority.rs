use crate::{
    account::exchange,
    artist_review_fixture::origin_counts,
    credit_member_identity_reset_fixture::accepted,
    credit_member_review_preview_fixture::{Context, Library, private},
    provider_fixture::ProviderServer,
};
use axum::http::{HeaderMap, StatusCode};
use hito_core::{
    MetadataProvider,
    test_support::{artist_catalog_discovery_service as catalog, playlist_service},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Notify, task::JoinSet};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::{Layer, layer::Context as TraceContext, prelude::*, registry::LookupSpan};
use uuid::Uuid;

pub(crate) type Response = (StatusCode, HeaderMap, Value);

pub(crate) struct Member {
    pub(crate) path: String,
    pub(crate) request: Value,
}

pub(crate) struct Fixture {
    library: Library,
    pub(crate) owned: Option<Context>,
    origin: Option<ProviderServer>,
    pub(crate) member: Option<Member>,
    pub(crate) hold: ReadFence,
    pub(crate) requests: JoinSet<Response>,
    pub(crate) deletions: JoinSet<Response>,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        Self {
            library: Library::new(),
            owned: None,
            origin: None,
            member: None,
            hold: ReadFence::new(),
            requests: JoinSet::new(),
            deletions: JoinSet::new(),
        }
    }

    pub(crate) async fn prepare(&mut self) {
        self.owned = Some(self.library.prepare().await);
        self.origin = Some(ProviderServer::start().await);
        let owned = self.owned.as_ref().unwrap();
        let origin = self.origin.as_ref().unwrap();
        let member_account = owned.peer.get(&owned.app, "/api/v1/me").await;
        assert_eq!(member_account["admin"], false);
        assert_eq!(member_account["username"], "member-preview-peer");
        origin.reply("v1/manifest", json!({
            "id": "org.hito.member-reset-authority", "name": "Owned reset authority metadata",
            "protocol_version": 1, "capabilities": ["playlists"],
        })).await;
        playlist_service::playlist(origin, "owned-reset-authority", "owned-reset-authority:1", vec![json!({
            "occurrence_id": "owned-reset-authority-occurrence", "item": {"kind": "recording", "recording": {
                "id": "owned-reset-authority-recording", "title": "Owned reset authority recording",
                "artist": "Owned reset authority Main feat. Owned reset authority Featured", "duration_ms": 100_000,
                "artists": [{"name": "Owned reset authority Main", "role": "main", "join": " feat. "},
                    {"name": "Owned reset authority Featured", "role": "featured"}],
            }}
        })]).await;
        let service = exchange(
            &owned.app,
            "POST",
            "/api/v1/plugins/services",
            json!({"name": "Owned reset authority metadata", "base_url": origin.url}),
            owned.account.headers(),
        )
        .await;
        assert_eq!(
            service.0,
            StatusCode::CREATED,
            "POST /api/v1/plugins/services returned HTTP {}",
            service.0
        );
        private(&service.1);
        let installation = exchange(&owned.app, "POST", "/api/v1/plugins", json!({
            "service_id": service.2["id"], "service_revision": service.2["revision"], "bearer_token": null,
        }), owned.peer.headers()).await;
        assert_eq!(
            installation.0,
            StatusCode::CREATED,
            "POST /api/v1/plugins returned HTTP {}",
            installation.0
        );
        private(&installation.1);
        let receipt = accepted(&owned.app, &owned.peer, "POST",
            &format!("/api/v1/plugins/{}/playlists/import", installation.2["id"].as_str().unwrap()),
            json!({"request_id": Uuid::new_v4(), "installation_revision": installation.2["revision"],
                "playlist_reference": "https://provider.invalid/owned-reset-authority"})).await;
        let playlist = receipt["playlist_id"].as_str().unwrap();
        let copy = owned
            .peer
            .get(&owned.app, &format!("/api/v1/playlists/{playlist}"))
            .await;
        assert_eq!(copy["entries"].as_array().unwrap().len(), 1);
        let recording = copy["entries"][0]["recording_id"].as_str().unwrap();
        let metadata = owned
            .peer
            .get(
                &owned.app,
                &format!("/api/v1/recordings/{recording}/metadata"),
            )
            .await;
        assert_eq!(metadata["credited_artists"][1]["role"], "featured");
        let artist = metadata["credited_artists"][1]["artist_id"]
            .as_str()
            .unwrap();
        let settings = owned.account.get(&owned.app, "/api/v1/plugins").await;
        let current = settings["installations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == owned.context["provider"]["installation_id"])
            .unwrap();
        let catalog_service = settings["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == current["service_id"])
            .unwrap();
        let catalog_installation = exchange(&owned.app, "POST", "/api/v1/plugins", json!({
            "service_id": catalog_service["id"], "service_revision": catalog_service["revision"],
            "bearer_token": catalog::TOKEN,
        }), owned.peer.headers()).await;
        assert_eq!(
            catalog_installation.0,
            StatusCode::CREATED,
            "POST /api/v1/plugins returned HTTP {}",
            catalog_installation.0
        );
        private(&catalog_installation.1);
        let selected = MetadataProvider::HttpPlugin {
            installation_id: catalog_installation.2["id"].as_str().unwrap().into(),
        };
        let policy = owned
            .peer
            .get(&owned.app, "/api/v1/metadata/settings")
            .await;
        accepted(
            &owned.app,
            &owned.peer,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": policy["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
        let prefix = format!("/api/v1/recordings/{recording}/credit-members/{artist}");
        let context = accepted(
            &owned.app,
            &owned.peer,
            "POST",
            &format!("{prefix}/artist-review/context"),
            json!({"provider": selected}),
        )
        .await;
        assert_eq!(context["account_id"], owned.peer.id);
        assert_eq!(context["artist_id"], artist);
        assert_eq!(context["role"], "featured");
        let association = accepted(
            &owned.app,
            &owned.peer,
            "POST",
            &format!("{prefix}/artist-review/confirm"),
            json!({"context": context, "provider_artist_id": catalog::SECOND_ARTIST}),
        )
        .await;
        let path = format!("{prefix}/artist-identity");
        let chosen = accepted(
            &owned.app,
            &owned.peer,
            "PUT",
            &path,
            json!({
                "request_id": Uuid::new_v4(), "expected_revision": 0,
                "association_id": association["id"], "context": context,
            }),
        )
        .await;
        assert_eq!(chosen["revision"], 1);
        assert_eq!(chosen["selection"]["association_id"], association["id"]);
        let current = owned.peer.get(&owned.app, &path).await;
        assert_eq!(current["state"]["status"], "selected");
        assert_eq!(current["revision"], 1);
        self.member = Some(Member {
            path,
            request: json!({"request_id": Uuid::new_v4(), "expected_revision": 1}),
        });
    }

    pub(crate) fn begin_reset(&mut self) {
        let owned = self.owned.as_ref().unwrap();
        let member = self.member.as_ref().unwrap();
        let app = owned.app.clone();
        let headers = owned.peer.headers();
        let path = format!("{}/reset", member.path);
        let body = member.request.clone();
        hito_testkit::logs::anchor_callsite_interest();
        let subscriber = tracing_subscriber::registry().with(self.hold.clone());
        self.requests.spawn(
            async move { exchange(&app, "POST", &path, body, headers).await }
                .with_subscriber(subscriber),
        );
    }

    pub(crate) fn begin_delete(&mut self) {
        let owned = self.owned.as_ref().unwrap();
        let app = owned.app.clone();
        let headers = owned.account.headers();
        let path = format!("/api/v1/users/{}", owned.peer.id);
        self.deletions.spawn(async move {
            exchange(
                &app,
                "DELETE",
                &path,
                json!({"username": "member-preview-peer"}),
                headers,
            )
            .await
        });
    }

    pub(crate) async fn counts(&self) -> Value {
        json!({"catalog": self.library.provider.as_ref().unwrap().requests().len(),
            "unexpected": self.library.provider.as_ref().unwrap().unexpected(),
            "original": self.library.origin_counts().await,
            "extra": origin_counts(self.origin.as_ref().unwrap()).await})
    }

    pub(crate) async fn retire(&mut self) {
        self.hold.release();
        self.requests.abort_all();
        self.deletions.abort_all();
        let joined = tokio::time::timeout(Duration::from_secs(3), async {
            while self.requests.join_next().await.is_some() {}
            while self.deletions.join_next().await.is_some() {}
        })
        .await
        .is_ok();
        drop(self.owned.take());
        if let Some(origin) = self.origin.take() {
            origin.shutdown().await;
        }
        self.library.retire().await;
        assert!(
            joined,
            "Owned reset and administrator deletion requests must join"
        );
    }
}

#[derive(Clone)]
pub(crate) struct ReadFence(Arc<FenceState>);

#[derive(Default)]
struct FenceState {
    entered: Notify,
    used: AtomicBool,
    waiting: AtomicBool,
    returned: AtomicBool,
    current: AtomicBool,
    transaction_open: AtomicBool,
    transaction_seen: AtomicBool,
    released: Mutex<bool>,
    changed: Condvar,
}

#[derive(Default)]
struct Fields {
    point: bool,
    current: bool,
    transaction_open: bool,
    transaction_seen: bool,
}

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "point" {
            self.point = value == "credit_member_identity_reset_fresh_session";
        }
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        match field.name() {
            "snapshot_current" => self.current = value,
            "transaction_open" => {
                self.transaction_open = value;
                self.transaction_seen = true;
            }
            _ => {}
        }
    }
}

impl<S> Layer<S> for ReadFence
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _: TraceContext<'_, S>) {
        if event.metadata().target() != "hito_core::authority_snapshot_test" {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if !fields.point || self.0.used.swap(true, Ordering::SeqCst) {
            return;
        }
        self.0.current.store(fields.current, Ordering::SeqCst);
        self.0
            .transaction_open
            .store(fields.transaction_open, Ordering::SeqCst);
        self.0
            .transaction_seen
            .store(fields.transaction_seen, Ordering::SeqCst);
        tokio::task::block_in_place(|| {
            self.0.waiting.store(true, Ordering::SeqCst);
            self.0.entered.notify_one();
            let released = self.0.released.lock().unwrap();
            let (released, timed) = self
                .0
                .changed
                .wait_timeout_while(released, Duration::from_secs(4), |released| !*released)
                .unwrap();
            let explicitly_released = *released;
            drop(released);
            self.0.waiting.store(false, Ordering::SeqCst);
            assert!(
                explicitly_released && !timed.timed_out(),
                "The owned member reset fence must be explicitly released"
            );
            self.0.returned.store(true, Ordering::SeqCst);
        });
    }
}

impl ReadFence {
    fn new() -> Self {
        Self(Arc::new(FenceState::default()))
    }
    pub(crate) async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.0.entered.notified())
            .await
            .unwrap();
        assert!(self.0.current.load(Ordering::SeqCst));
        assert!(self.0.transaction_seen.load(Ordering::SeqCst));
        assert!(!self.0.transaction_open.load(Ordering::SeqCst));
        self.assert_held();
    }
    pub(crate) fn assert_held(&self) {
        assert!(self.0.waiting.load(Ordering::SeqCst));
    }
    pub(crate) fn assert_returned(&self) {
        assert!(self.0.returned.load(Ordering::SeqCst));
    }
    pub(crate) fn release(&self) {
        *self.0.released.lock().unwrap() = true;
        self.0.changed.notify_all();
    }
}
