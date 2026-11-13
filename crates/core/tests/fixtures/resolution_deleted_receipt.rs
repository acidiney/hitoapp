use hito_core::test_support::{database_owner::DatabaseOwner, playlist_service};
use hito_core::{
    AccountDeletion, Actor, CoreConfig, Credentials, DeleteAccount, Error, HostDeviceId,
    ImportProviderPlaylist, InstallPlugin, OfferPluginService, PluginInstallation, PluginService,
    ResolveSource, Services, SourceResolution,
};
use hito_plugin_sdk::{Manifest, SearchRequest};
use hito_testkit::fake_plugin::{FakePlugin, Handled, Reply};
use serde_json::json;
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

const USERNAME: &str = "owned-deleted-listener";
const PASSWORD: &str = "owned deleted listener password";
const SEARCH: &str = "v1/search";

pub(crate) struct Fixture {
    directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    hito: Option<Services>,
    primary: Option<playlist_service::ProviderServer>,
    provider: Option<FakePlugin>,
    admin: Option<Actor>,
    listener: Option<Actor>,
    listener_id: Option<String>,
    token: Option<String>,
    recording: Option<String>,
    request_id: Uuid,
    hold: ReadFence,
    requests: JoinSet<hito_core::Result<SourceResolution>>,
    deletions: JoinSet<hito_core::Result<AccountDeletion>>,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let database = DatabaseOwner::retain(directory.clone());
        Self {
            directory,
            database,
            hito: None,
            primary: None,
            provider: None,
            admin: None,
            listener: None,
            listener_id: None,
            token: None,
            recording: None,
            request_id: Uuid::new_v4(),
            hold: ReadFence::new(),
            requests: JoinSet::new(),
            deletions: JoinSet::new(),
        }
    }

    pub(crate) async fn prepare(&mut self) {
        let mut config = CoreConfig::new(
            self.directory.path().join("hito.db"),
            self.directory.path().join("music"),
            HostDeviceId::new("resolution-deleted-receipt").unwrap(),
        );
        self.database.attach(&mut config);
        self.hito = Some(Box::pin(hito_core::open(config)).await.unwrap());
        let hito = self.hito.as_ref().unwrap();
        let session = hito
            .identity
            .setup(credentials("owned-receipt-admin"))
            .await
            .unwrap();
        self.admin = Some(hito.identity.authenticate(&session.token).await.unwrap());
        let admin = self.admin.as_ref().unwrap();
        hito.identity
            .create_user(admin, credentials(USERNAME))
            .await
            .unwrap();
        let session = hito.identity.login(credentials(USERNAME)).await.unwrap();
        assert!(
            !session.user.admin,
            "the deletable listener must be a member"
        );
        self.listener_id = Some(session.user.id.clone());
        self.listener = Some(hito.identity.authenticate(&session.token).await.unwrap());
        self.token = Some(session.token);
        self.primary = Some(playlist_service::ProviderServer::start().await);
        let primary = self.primary.as_ref().unwrap();
        primary
            .reply(
                "v1/manifest",
                json!({
                    "id":"org.hito.deleted-receipt-primary", "name":"Owned receipt metadata",
                    "protocol_version":1, "capabilities":["playlists"],
                }),
            )
            .await;
        playlist_service::playlist(primary, "owned-receipt-playlist", "owned-receipt-snapshot", vec![json!({
            "occurrence_id":"owned-receipt-occurrence", "item":{"kind":"recording", "recording":{
                "id":"owned-receipt-recording", "title":"Owned tune", "artist":"Owned performer", "duration_ms":1000,
                "release":{"id":"owned-receipt-album", "title":"Owned studio album", "artist":"Owned performer"},
                "revision":"owned-receipt-primary-r1",
            }}
        })]).await;
        let service = offer(hito, admin, &primary.url).await;
        let listener = self.listener.as_ref().unwrap();
        let member_installation = install(hito, listener, &service).await;
        let (_, recording) = import(hito, listener, &member_installation).await;
        self.recording = Some(recording);
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"org.hito.deleted-receipt-audio", "name":"Owned unavailable receipt audio",
            "protocol_version":1, "capabilities":["search","sources"],
        }))
        .unwrap();
        self.provider = Some(
            FakePlugin::builder(manifest)
                .require_protocol()
                .require_json()
                .strict(4)
                .post(SEARCH, |request| {
                    let valid = serde_json::from_slice::<SearchRequest>(&request.body).is_ok_and(
                        |request| {
                            request.validate().is_ok()
                                && request.query == "Owned performer Owned tune"
                                && request.cursor.is_none()
                                && request.limit == 20
                        },
                    );
                    Reply::Status(if valid {
                        axum::http::StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        axum::http::StatusCode::BAD_REQUEST
                    })
                })
                .start()
                .await,
        );
        let service = offer(hito, admin, &self.provider.as_ref().unwrap().url).await;
        let installation = install(hito, listener, &service).await;
        assert!(
            installation
                .manifest
                .capabilities
                .contains(&hito_plugin_sdk::Capability::Search)
        );
        assert!(
            installation
                .manifest
                .capabilities
                .contains(&hito_plugin_sdk::Capability::Sources)
        );
        assert!(
            hito.catalog
                .sources(listener, self.recording.as_ref().unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    }

    pub(crate) async fn exercise(&mut self) -> hito_core::Result<SourceResolution> {
        let hito = self.hito.as_ref().unwrap();
        let listener = self.listener.as_ref().unwrap();
        let recording = self.recording.as_ref().unwrap();
        let accepted = hito
            .catalog
            .resolve_source_until(
                listener,
                recording,
                ResolveSource {
                    request_id: self.request_id,
                },
                std::future::pending(),
            )
            .await
            .unwrap_or_else(|_| {
                panic!("the first valid listener resolution must be accepted before deletion")
            });
        let decision = serde_json::to_value(&accepted).unwrap();
        assert!(accepted.request_id == self.request_id && accepted.recording_id == *recording);
        assert!(accepted.sources.is_empty());
        assert!(
            decision["outcome"] == "provider_unavailable"
                && decision["failure"]["reason"] == "provider_unavailable"
                && decision["failure"]["retryable"] == true,
            "the accepted503 must retain its typed transient failure decision"
        );
        self.assert_protocol().await;
        let provider_calls = self.provider.as_ref().unwrap().requests().len();
        let hito = hito.clone();
        let listener = listener.clone();
        let recording = recording.clone();
        let request_id = self.request_id;
        hito_testkit::logs::anchor_callsite_interest();
        let subscriber = tracing_subscriber::registry().with(self.hold.clone());
        self.requests.spawn(
            async move {
                hito.catalog
                    .resolve_source_until(
                        &listener,
                        &recording,
                        ResolveSource { request_id },
                        std::future::pending(),
                    )
                    .await
            }
            .with_subscriber(subscriber),
        );
        self.hold.entered().await;
        let hito = self.hito.as_ref().unwrap().clone();
        let admin = self.admin.as_ref().unwrap().clone();
        let listener_id = self.listener_id.as_ref().unwrap().clone();
        self.deletions.spawn(async move {
            hito.identity
                .delete_account(
                    &admin,
                    &listener_id,
                    DeleteAccount {
                        username: USERNAME.into(),
                    },
                )
                .await
        });
        let deleted = tokio::time::timeout(Duration::from_secs(3), self.deletions.join_next())
            .await
            .unwrap_or_else(|_| {
                panic!("administrator deletion must commit while replay remains held")
            })
            .unwrap()
            .unwrap()
            .unwrap_or_else(|_| {
                panic!("the administrator must delete the member through Identity")
            });
        assert!(deleted.sessions_revoked > 0 && deleted.rows_removed > 0);
        self.hold.assert_held();
        assert!(
            matches!(
                self.hito
                    .as_ref()
                    .unwrap()
                    .identity
                    .authenticate(self.token.as_ref().unwrap())
                    .await,
                Err(Error::Unauthorized)
            ),
            "fresh authentication must observe the committed account deletion"
        );
        self.hold.assert_held();
        self.hold.release();
        let replay = tokio::time::timeout(Duration::from_secs(3), self.requests.join_next())
            .await
            .unwrap_or_else(|_| panic!("the released receipt replay must join"))
            .unwrap()
            .unwrap();
        self.hold.assert_returned();
        assert_eq!(
            self.provider.as_ref().unwrap().requests().len(),
            provider_calls
        );
        self.assert_protocol().await;
        replay
    }

    async fn assert_protocol(&self) {
        let provider = self.provider.as_ref().unwrap();
        assert!(
            provider.requests().len() == 2
                && provider.requests_for(SEARCH).len() == 1
                && provider.requests_for("v1/recordings/detail").is_empty()
                && provider.requests_for("v1/sources").is_empty()
                && provider.requests_for("v1/audio").is_empty()
                && provider.health_requests().is_empty()
                && provider.unexpected() == 0
                && provider
                    .requests()
                    .iter()
                    .all(|request| request.handled == Handled::Yes),
            "only the approved manifest and one failed search may reach the audio fake"
        );
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
        let provider_joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        drop(self.provider.take());
        if let Some(primary) = self.primary.take() {
            primary.shutdown().await;
        }
        let closed = match self.hito.take() {
            Some(hito) => {
                let closed = hito.catalog_replication.close().await.is_ok();
                drop(hito);
                closed
            }
            None => true,
        };
        let retirement = self.database.retire(Duration::from_secs(3)).await;
        assert!(
            joined,
            "owned replay and deletion tasks must join before the target assertion"
        );
        assert!(
            provider_joined && closed,
            "owned provider and Core workers must finish"
        );
        assert!(
            retirement.resources_retired && !retirement.deadline_exceeded,
            "owned SQLite resources must retire before the target assertion"
        );
    }
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

async fn offer(hito: &Services, admin: &Actor, base_url: &str) -> PluginService {
    hito.plugins
        .offer_service(
            admin,
            OfferPluginService {
                name: "Owned deleted receipt service".into(),
                base_url: base_url.into(),
            },
        )
        .await
        .unwrap()
}

async fn install(hito: &Services, actor: &Actor, service: &PluginService) -> PluginInstallation {
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
                accept_manifest_digest: None,
            },
        )
        .await
        .unwrap()
}

async fn import(
    hito: &Services,
    actor: &Actor,
    installation: &PluginInstallation,
) -> (String, String) {
    let imported = hito
        .collection
        .import_provider_playlist(
            actor,
            &installation.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-receipt-playlist".into(),
            },
        )
        .await
        .unwrap();
    let playlist = hito
        .collection
        .playlist(actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 1);
    assert!(!playlist.entries[0].available);
    (
        imported.playlist_id.to_string(),
        playlist.entries[0].recording_id.clone().unwrap(),
    )
}

#[derive(Clone)]
struct ReadFence(Arc<FenceState>);

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
            self.point = value == "source_resolution_final_access_before_failure";
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
                "The owned receipt replay fence must be explicitly released"
            );
            self.0.returned.store(true, Ordering::SeqCst);
        });
    }
}

impl ReadFence {
    fn new() -> Self {
        Self(Arc::new(FenceState::default()))
    }
    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.0.entered.notified())
            .await
            .unwrap();
        assert!(self.0.current.load(Ordering::SeqCst));
        assert!(self.0.transaction_seen.load(Ordering::SeqCst));
        assert!(!self.0.transaction_open.load(Ordering::SeqCst));
        self.assert_held();
    }
    fn assert_held(&self) {
        assert!(self.0.waiting.load(Ordering::SeqCst));
    }
    fn assert_returned(&self) {
        assert!(self.0.returned.load(Ordering::SeqCst));
    }
    fn release(&self) {
        *self.0.released.lock().unwrap() = true;
        self.0.changed.notify_all();
    }
}
