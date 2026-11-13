use crate::artwork_thousand_corpus::Corpus;
use crate::artwork_thousand_provider as provider;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{
    CoreConfig, HostDeviceId, Services,
    test_support::{database_owner::DatabaseOwner, support},
};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    Connection,
    sqlite::{SqliteConnectOptions, SqliteConnection},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
use tower::ServiceExt;

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap()
    }
}

pub struct Library {
    directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    services: Option<Services>,
    app: Option<Router>,
    provider: Option<FakePlugin>,
    inspection: Option<tokio::sync::Mutex<SqliteConnection>>,
    inspection_owner: InspectionOwner,
    inspection_opening: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
    original_audio: Vec<u8>,
    original_cover: Vec<u8>,
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
            provider: None,
            inspection: None,
            inspection_owner: InspectionOwner::new(),
            inspection_opening: None,
            original_audio: Vec::new(),
            original_cover: include_bytes!("artwork-normalization-1024x768.png").to_vec(),
        }
    }

    pub fn corpus_directory(&self) -> std::path::PathBuf {
        self.directory.path().join("corpus")
    }

    pub async fn open(&mut self, corpus: &Corpus) {
        let music = self.directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        self.original_audio = support::write_wav(&music.join("Owned.wav"), 0);
        std::fs::write(music.join("cover.png"), &self.original_cover).unwrap();
        for path in [
            music.join("Owned.wav"),
            music.join("cover.png"),
            music.clone(),
        ] {
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(path, permissions).unwrap();
        }
        let mut config = CoreConfig::new(
            self.directory.path().join("hito.db"),
            music,
            HostDeviceId::new("artwork-thousand").unwrap(),
        );
        self.database.attach(&mut config);
        self.services = Some(hito_core::open(config).await.unwrap());
        self.open_inspection(
            SqliteConnectOptions::new()
                .filename(self.directory.path().join("hito.db"))
                .read_only(true),
        )
        .await
        .unwrap();
        self.app = Some(hito_server::router(self.hito().clone()));
        self.provider = Some(provider::start(corpus.directory()).await);
    }

    async fn open_inspection(&mut self, options: SqliteConnectOptions) -> Result<(), sqlx::Error> {
        let options = self.inspection_owner.options(options, &self.directory);
        let connection = SqliteConnection::connect_with(&options).await?;
        if let Some((entered, release)) = self.inspection_opening.take() {
            let _ = entered.send(());
            let _ = release.await;
        }
        self.inspection = Some(tokio::sync::Mutex::new(connection));
        Ok(())
    }

    async fn close_inspection(&mut self) -> bool {
        self.inspection_owner.close_gate();
        let mut closing = Box::pin(async {
            let closed = if let Some(inspection) = self.inspection.take() {
                inspection.into_inner().close().await.is_ok()
            } else {
                true
            };
            self.inspection_owner.wait_retired().await && closed
        });
        match tokio::time::timeout(Duration::from_secs(3), &mut closing).await {
            Ok(result) => result,
            Err(_) => {
                let _ = closing.await;
                false
            }
        }
    }

    pub async fn inspect(&self, owner: &str) -> Storage {
        let mut connection = self.inspection.as_ref().unwrap().lock().await;
        let account: (i64, i64) = sqlx::query_as(
            "SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets WHERE user_id = ?",
        )
        .bind(owner)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        let global: (i64, i64) =
            sqlx::query_as("SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets")
                .fetch_one(&mut *connection)
                .await
                .unwrap();
        let pending: (i64, i64, i64, i64) = sqlx::query_as("SELECT count(*),coalesce(sum(reservation_bytes),0),coalesce(sum(user_id = ?),0),coalesce(sum(CASE WHEN user_id = ? THEN reservation_bytes ELSE 0 END),0) FROM artwork_fetches WHERE status IN ('queued','running')")
            .bind(owner).bind(owner).fetch_one(&mut *connection).await.unwrap();
        let references: Vec<(String,String,String)> = sqlx::query_as("SELECT user_id,release_id,content_hash FROM release_artwork ORDER BY user_id,release_id")
            .fetch_all(&mut *connection).await.unwrap();
        let exact_references: Vec<(String,String,String,String,String,String)> = sqlx::query_as("SELECT user_id,'release',release_id,observation_id,fetch_id,content_hash FROM release_artwork UNION ALL SELECT user_id,'recording',recording_id,observation_id,fetch_id,content_hash FROM recording_artwork")
            .fetch_all(&mut *connection).await.unwrap();
        let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        assert!(
            foreign_keys.is_empty(),
            "All owned artwork and catalog references must remain valid"
        );
        let dangling: i64 = sqlx::query_scalar("SELECT count(*) FROM retained_artwork_references r LEFT JOIN artwork_assets a ON a.user_id = r.user_id AND a.content_hash = r.content_hash WHERE a.content_hash IS NULL")
            .fetch_one(&mut *connection).await.unwrap();
        assert_eq!(dangling, 0);
        let invalid_retained: i64 = sqlx::query_scalar("SELECT count(*) FROM retained_artwork_references r JOIN artwork_fetches f ON f.id = r.fetch_id WHERE f.status != 'ready' OR f.retained != 1")
            .fetch_one(&mut *connection).await.unwrap();
        assert_eq!(invalid_retained, 0);
        assert!(global.1 + pending.1 <= 256 * 1024 * 1024);
        assert!(global.0 + pending.0 <= 8192);
        assert!(pending.0 <= 128 && pending.2 <= 16);
        Storage {
            account_assets: account.0,
            account_bytes: account.1,
            global_assets: global.0,
            global_bytes: global.1,
            pending: pending.0,
            reserved: pending.1,
            account_pending: pending.2,
            account_reserved: pending.3,
            references,
            exact_references,
        }
    }

    pub async fn audit_asset_keys(&self) {
        let mut connection = self.inspection.as_ref().unwrap().lock().await;
        let assets: Vec<(String, Vec<u8>)> =
            sqlx::query_as("SELECT content_hash,bytes FROM artwork_assets")
                .fetch_all(&mut *connection)
                .await
                .unwrap();
        for (hash, bytes) in assets {
            assert!(
                hash == format!("{:x}", Sha256::digest(bytes)),
                "Owned asset keys must match their retained bytes"
            );
        }
    }

    pub async fn account_id(&self, token: &str) -> String {
        self.hito()
            .identity
            .authenticate(token)
            .await
            .unwrap()
            .user()
            .id
            .clone()
    }

    pub fn hito(&self) -> &Services {
        self.services.as_ref().unwrap()
    }
    pub fn provider(&self) -> &FakePlugin {
        self.provider.as_ref().unwrap()
    }

    pub async fn exchange(
        &self,
        token: Option<&str>,
        method: &str,
        path: &str,
        body: Value,
    ) -> Response {
        let limit = if method == "POST"
            && path.ends_with("/playlists/import")
            && body["playlist_reference"] == "https://provider.invalid/capacity/bulk"
        {
            60
        } else {
            5
        };
        tokio::time::timeout(Duration::from_secs(limit), async {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("x-hito-request", "1");
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            let response = self
                .app
                .as_ref()
                .unwrap()
                .clone()
                .oneshot(request.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec();
            Response {
                status,
                headers,
                bytes,
            }
        })
        .await
        .expect("The owned artwork HTTP request must finish")
    }

    pub async fn get(&self, token: &str, path: &str) -> Value {
        let response = self.exchange(Some(token), "GET", path, Value::Null).await;
        assert_eq!(response.status, StatusCode::OK);
        response.json()
    }

    pub fn assert_originals(&self) {
        let music = self.directory.path().join("music");
        assert!(
            Sha256::digest(std::fs::read(music.join("Owned.wav")).unwrap())
                == Sha256::digest(&self.original_audio),
            "The owned original audio must remain unchanged"
        );
        assert!(
            Sha256::digest(std::fs::read(music.join("cover.png")).unwrap())
                == Sha256::digest(&self.original_cover),
            "The owned original cover must remain unchanged"
        );
    }

    pub async fn close(mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let music = self.directory.path().join("music");
            if music.exists() {
                std::fs::set_permissions(music, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        let provider_joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        drop(self.app.take());
        let inspection_closed = self.close_inspection().await;
        let closed = if let Some(hito) = self.services.take() {
            let playback = hito.playback.close().await;
            let replicas = hito.replicas.close().await;
            let replication = hito.catalog_replication.close().await;
            drop(hito);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
        let retired = self.database.retire(Duration::from_secs(3)).await;
        assert!(provider_joined && inspection_closed && closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
    }
}

#[derive(Clone)]
pub struct Storage {
    pub account_assets: i64,
    pub account_bytes: i64,
    pub global_assets: i64,
    pub global_bytes: i64,
    pub pending: i64,
    pub reserved: i64,
    pub account_pending: i64,
    pub account_reserved: i64,
    pub references: Vec<(String, String, String)>,
    pub exact_references: Vec<(String, String, String, String, String, String)>,
}

impl Storage {
    pub fn report(&self, stage: &str) {
        eprintln!(
            "artwork_thousand stage={stage} account_assets={} account_bytes={} global_assets={} global_bytes={} pending={} reserved={} account_pending={} account_reserved={}",
            self.account_assets,
            self.account_bytes,
            self.global_assets,
            self.global_bytes,
            self.pending,
            self.reserved,
            self.account_pending,
            self.account_reserved
        );
    }
}

struct InspectionOwner {
    admission: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    retired: oneshot::Receiver<()>,
    worker_expected: Option<bool>,
    result: Option<bool>,
    retirement_hold: Option<Arc<inspection_ownership::RetirementHold>>,
}

impl InspectionOwner {
    fn new() -> Self {
        let (finished, retired) = oneshot::channel();
        Self {
            admission: Arc::new(Mutex::new(Some(finished))),
            retired,
            worker_expected: None,
            result: None,
            retirement_hold: None,
        }
    }

    fn options(
        &self,
        options: SqliteConnectOptions,
        directory: &Arc<tempfile::TempDir>,
    ) -> SqliteConnectOptions {
        let admission = self.admission.clone();
        let directory = Arc::downgrade(directory);
        let retirement_hold = self.retirement_hold.clone();
        options.worker_guard(move || {
            let directory = directory.upgrade().ok_or(sqlx::Error::PoolClosed)?;
            let finished = admission
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .ok_or(sqlx::Error::PoolClosed)?;
            Ok(InspectionLease {
                directory: Some(directory),
                finished: Some(finished),
                retirement_hold: retirement_hold.clone(),
                factory_thread: std::thread::current().id(),
            })
        })
    }

    fn close_gate(&mut self) {
        if self.worker_expected.is_none() {
            let unused = self
                .admission
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            self.worker_expected = Some(unused.is_none());
            drop(unused);
        }
    }

    async fn wait_retired(&mut self) -> bool {
        if let Some(result) = self.result {
            return result;
        }
        let completed = (&mut self.retired).await.is_ok();
        let result = self.worker_expected == Some(completed);
        self.result = Some(result);
        result
    }
}

struct InspectionLease {
    factory_thread: std::thread::ThreadId,
    directory: Option<Arc<tempfile::TempDir>>,
    finished: Option<oneshot::Sender<()>>,
    retirement_hold: Option<Arc<inspection_ownership::RetirementHold>>,
}

impl Drop for InspectionLease {
    fn drop(&mut self) {
        if std::thread::current().id() != self.factory_thread
            && let Some(hold) = &self.retirement_hold
        {
            hold.wait();
        }
        drop(self.directory.take());
        if let Some(finished) = self.finished.take() {
            let _ = finished.send(());
        }
    }
}

mod inspection_ownership {
    use super::*;
    use futures_util::FutureExt;
    use hito_core::test_support::hang_guard::hang_guard;
    use std::{
        future::Future,
        panic::{AssertUnwindSafe, resume_unwind},
        sync::Condvar,
        task::Poll,
    };
    use tokio::sync::Semaphore;

    pub(super) struct RetirementHold {
        entered: Semaphore,
        released: Mutex<bool>,
        changed: Condvar,
    }

    impl RetirementHold {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                entered: Semaphore::new(0),
                released: Mutex::new(false),
                changed: Condvar::new(),
            })
        }

        pub(super) fn wait(&self) {
            self.entered.add_permits(1);
            let mut released = self
                .released
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while !*released {
                released = self
                    .changed
                    .wait(released)
                    .unwrap_or_else(|error| error.into_inner());
            }
        }

        async fn entered(&self) {
            let permit = self.entered.acquire().await.unwrap();
            permit.forget();
        }

        fn release(&self) {
            *self
                .released
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = true;
            self.changed.notify_all();
        }
    }

    #[tokio::test]
    async fn caller_thread_inspection_lease_drop_releases_ownership_without_waiting_on_worker_hold()
    {
        let library = hang_guard("owned inspection caller-drop setup", async {
            Library::retain()
        })
        .await;
        let weak = Arc::downgrade(&library.directory);
        let directory = library.directory.clone();
        let admission = library.inspection_owner.admission.clone();
        let hold = RetirementHold::new();
        let caller_hold = hold.clone();
        let mut dropping = tokio::task::spawn_blocking(move || {
            let finished = admission
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .unwrap();
            drop(InspectionLease {
                directory: Some(directory),
                finished: Some(finished),
                retirement_hold: Some(caller_hold),
                factory_thread: std::thread::current().id(),
            });
        });
        let completed = tokio::time::timeout(Duration::from_secs(5), &mut dropping).await;
        let finished_without_release = completed.is_ok();
        hold.release();
        let joined = match completed {
            Ok(joined) => joined,
            Err(_) => dropping.await,
        };
        let worker_hold_not_entered = hold.entered.available_permits() == 0;
        library.close().await;
        assert!(
            finished_without_release && joined.is_ok() && worker_hold_not_entered,
            "Caller-thread lease destruction must release its directory and completion without entering the worker retirement hold"
        );
        assert!(
            weak.upgrade().is_none(),
            "Caller-thread lease destruction must release its temporary directory without a cycle"
        );
    }

    #[tokio::test]
    async fn failed_inspection_open_retains_its_actual_sqlite_worker_until_retirement() {
        control(false).await;
    }

    #[tokio::test]
    async fn cancelled_inspection_open_retains_its_actual_sqlite_worker_until_retirement() {
        control(true).await;
    }

    async fn control(cancel: bool) {
        let mut library = Library::retain();
        let weak = Arc::downgrade(&library.directory);
        let baseline = Arc::strong_count(&library.directory);
        let hold = RetirementHold::new();
        library.inspection_owner.retirement_hold = Some(hold.clone());
        let outcome = AssertUnwindSafe(Box::pin(async {
            tokio::time::timeout(Duration::from_secs(30), Box::pin(async {
                let options = hang_guard("owned inspection file setup", async {
                    let path = library.directory.path().join("inspection-control.db");
                    std::fs::write(&path, []).unwrap();
                    SqliteConnectOptions::new().filename(path).read_only(true)
                }).await;
                let supported_failure = if cancel {
                    let (entered, arrived) = oneshot::channel();
                    let (_release, held) = oneshot::channel();
                    library.inspection_opening = Some((entered, held));
                    let mut opening = Box::pin(library.open_inspection(options));
                    hang_guard("owned inspection cancellation setup", async {
                        tokio::select! {
                            biased;
                            _ = &mut opening => panic!("The inspection opening control must reach its publication barrier"),
                            result = arrived => assert!(result.is_ok()),
                        }
                    }).await;
                    drop(opening);
                    true
                } else {
                    let result = hang_guard("owned inspection failure setup", library.open_inspection(options.pragma("foreign_keys", ")"))).await;
                    match result {
                        Err(sqlx::Error::Database(error)) => error.code().as_deref() == Some("1") && error.message().contains("syntax"),
                        _ => false,
                    }
                };
                hold.entered().await;
                let unpublished = library.inspection.is_none();
                let worker_owns_directory = Arc::strong_count(&library.directory) > baseline;
                let mut retirement = Box::pin(library.close_inspection());
                let waits_for_worker = std::future::poll_fn(|context| Poll::Ready(retirement.as_mut().poll(context).is_pending())).await;
                drop(retirement);
                (supported_failure, unpublished, worker_owns_directory, waits_for_worker)
            })).await.expect("The focused inspection ownership control must finish within its ordinary workflow deadline")
        })).catch_unwind().await;
        hold.release();
        library.close().await;
        let evidence = match outcome {
            Ok(evidence) => evidence,
            Err(payload) => resume_unwind(payload),
        };
        assert!(
            evidence.0 && evidence.1 && evidence.2 && evidence.3,
            "An unpublished failed or cancelled inspection must retain its actual worker and await its lease retirement"
        );
        assert!(
            weak.upgrade().is_none(),
            "Inspection ownership must release its temporary directory without a cycle"
        );
    }
}
