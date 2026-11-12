use crate::contract::{fixture, logging, server::Shutdown};
use crate::{Config, router};
use futures_util::FutureExt;
use hito_core::test_support::database_owner::{
    DatabaseObservation, DatabaseOwner, OpeningGate, RetirementGate,
};
use hito_core::{CoreConfig, HostDeviceId, Services};
use reqwest::Client;
use std::{
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, oneshot};

pub(super) struct Host {
    directory: Arc<tempfile::TempDir>,
    pub(super) provider: fixture::Provider,
    original: PathBuf,
    logs: logging::Capture,
    pub(super) plugin: fixture::Server,
    services: Option<Services>,
    server: Option<fixture::Server>,
    pub(super) client: Client,
    observation: Observation,
    database_owner: Option<DatabaseOwner>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Complete,
    Cancelled,
    Panicked,
    Deadline,
}

#[derive(Clone, Copy)]
pub(super) struct Report {
    pub(super) outcome: Outcome,
    pub(super) connections: Shutdown,
    pub(super) verified: bool,
    pub(super) jobs_settled: bool,
    pub(super) database_retired: bool,
    pub(super) database_deadline_exceeded: bool,
}

#[derive(Clone)]
pub(super) struct Observation {
    pub(super) directory: Arc<Mutex<Option<PathBuf>>>,
    pub(super) report: Arc<Mutex<Option<Report>>>,
    pub(super) servers: Arc<Mutex<Vec<url::Url>>>,
    pub(super) database: Arc<Mutex<Option<DatabaseObservation>>>,
    pub(super) database_ready: Arc<Semaphore>,
    pub(super) database_opening: Option<Arc<OpeningGate>>,
    pub(super) database_retirement: Option<Arc<RetirementGate>>,
    pub(super) database_deadline: Option<Duration>,
}

impl Default for Observation {
    fn default() -> Self {
        Self {
            directory: Arc::default(),
            report: Arc::default(),
            servers: Arc::default(),
            database: Arc::default(),
            database_ready: Arc::new(Semaphore::new(0)),
            database_opening: None,
            database_retirement: None,
            database_deadline: None,
        }
    }
}

pub(super) struct StartupGate {
    pub(super) entered: Semaphore,
    pub(super) release: Semaphore,
    pub(super) panic: bool,
}

impl StartupGate {
    pub(super) fn new(panic: bool) -> Self {
        Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            panic,
        }
    }
}

struct Startup {
    directory: Arc<tempfile::TempDir>,
    provider: Option<fixture::Provider>,
    plugin: Option<fixture::Server>,
    services: Option<Services>,
    server: Option<fixture::Server>,
    logs: logging::Capture,
    observation: Observation,
    database_owner: Option<DatabaseOwner>,
}

impl Startup {
    fn new(directory: Arc<tempfile::TempDir>, observation: Observation) -> Self {
        Self {
            directory,
            provider: None,
            plugin: None,
            services: None,
            server: None,
            logs: logging::Capture::default(),
            observation,
            database_owner: None,
        }
    }

    async fn initialize(&mut self, gate: Option<Arc<StartupGate>>) {
        let music = self.directory.path().join("music");
        std::fs::create_dir(&music).unwrap_or_else(|_| panic!("owned music directory failed"));
        self.provider = Some(fixture::Provider::start().await);
        self.observation
            .servers
            .lock()
            .unwrap()
            .push(self.provider.as_ref().unwrap().base.clone());
        std::fs::write(
            music.join("owned-original.wav"),
            &self.provider.as_ref().unwrap().wav,
        )
        .unwrap_or_else(|_| panic!("owned original fixture failed"));
        self.plugin = Some(
            fixture::Server::start_with_dispatch(
                router(Config::fixture(
                    fixture::CLIENT_ID.to_owned(),
                    self.provider.as_ref().unwrap().base.clone(),
                ))
                .unwrap_or_else(|_| panic!("Jamendo fixture router failed")),
                tracing::Dispatch::new(self.logs.clone()),
            )
            .await,
        );
        self.observation
            .servers
            .lock()
            .unwrap()
            .push(self.plugin.as_ref().unwrap().base.clone());
        self.database_owner = Some(database(self.directory.clone(), &self.observation));
        self.services =
            Some(open(self.directory.path(), self.database_owner.as_ref().unwrap()).await);
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            if gate.panic {
                panic!("owned partial startup control failure");
            }
            let _ = gate.release.acquire().await;
        }
        self.server = Some(
            fixture::Server::start(hito_server::router(self.services.as_ref().unwrap().clone()))
                .await,
        );
        self.observation
            .servers
            .lock()
            .unwrap()
            .push(self.server.as_ref().unwrap().base.clone());
    }

    fn complete(mut self) -> Host {
        Host {
            original: self.directory.path().join("music/owned-original.wav"),
            directory: self.directory,
            provider: self.provider.take().unwrap(),
            plugin: self.plugin.take().unwrap(),
            services: self.services.take(),
            server: self.server.take(),
            logs: self.logs,
            observation: self.observation,
            database_owner: self.database_owner.take(),
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_else(|_| panic!("owned host client failed")),
        }
    }

    async fn finish(mut self, outcome: Outcome) -> Report {
        let mut connections = Shutdown::default();
        if let Some(server) = self.server.take() {
            merge(&mut connections, server.shutdown_report().await);
        }
        if let Some(plugin) = self.plugin.take() {
            merge(&mut connections, plugin.shutdown_report().await);
        }
        if let Some(provider) = self.provider.take() {
            merge(&mut connections, provider.shutdown_report().await);
        }
        let (jobs_settled, retirement) = settle(
            self.services.take(),
            self.database_owner.take(),
            self.observation.database_deadline,
        )
        .await;
        Report {
            outcome,
            connections,
            verified: true,
            jobs_settled,
            database_retired: retirement.resources_retired,
            database_deadline_exceeded: retirement.deadline_exceeded,
        }
    }
}

impl Host {
    pub(super) fn server(&self) -> &fixture::Server {
        self.server.as_ref().unwrap()
    }
    pub(super) fn services(&self) -> &Services {
        self.services.as_ref().unwrap()
    }

    pub(super) async fn reopen(&mut self) {
        assert!(
            self.server.take().unwrap().shutdown().await,
            "host must stop before reopening"
        );
        let (jobs, retirement) = settle(
            self.services.take(),
            self.database_owner.take(),
            self.observation.database_deadline,
        )
        .await;
        assert!(
            jobs && retirement.resources_retired && !retirement.deadline_exceeded,
            "host resources must retire before reopening"
        );
        self.database_owner = Some(database(self.directory.clone(), &self.observation));
        self.services =
            Some(open(self.directory.path(), self.database_owner.as_ref().unwrap()).await);
        self.server =
            Some(fixture::Server::start(hito_server::router(self.services().clone())).await);
        self.observation
            .servers
            .lock()
            .unwrap()
            .push(self.server().base.clone());
    }

    pub(super) async fn hold_requests(&mut self, hold: Arc<RequestHold>) {
        assert!(
            self.server.take().unwrap().shutdown().await,
            "control host must stop before replacing its router"
        );
        let core = self.services().clone();
        let directory = self.directory.path().to_path_buf();
        let router = hito_server::router(core.clone()).layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let hold = hold.clone();
                let core = core.clone();
                let directory = directory.clone();
                async move {
                    hold.active
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let _held = HeldCore {
                        core,
                        directory,
                        hold: hold.clone(),
                    };
                    hold.entered.add_permits(1);
                    let _ = hold.release.acquire().await;
                    next.run(request).await
                }
            },
        ));
        self.server = Some(fixture::Server::start(router).await);
        self.observation
            .servers
            .lock()
            .unwrap()
            .push(self.server().base.clone());
    }

    async fn finish(mut self, outcome: Outcome) -> Report {
        let mut connections = Shutdown::default();
        if let Some(server) = self.server.take() {
            merge(&mut connections, server.shutdown_report().await);
        }
        merge(&mut connections, self.plugin.shutdown_report().await);
        let original_preserved =
            std::fs::read(self.original).is_ok_and(|bytes| bytes == self.provider.wav);
        let upstream_private = !self.provider.rejected_request();
        merge(&mut connections, self.provider.shutdown_report().await);
        let (jobs_settled, retirement) = settle(
            self.services.take(),
            self.database_owner.take(),
            self.observation.database_deadline,
        )
        .await;
        let captured = self.logs.text();
        let private = [
            fixture::CLIENT_ID,
            fixture::QUERY,
            fixture::TITLE,
            fixture::ARTIST,
            fixture::ALBUM,
            "private-",
            "http://",
            "https://",
            "127.0.0.1",
        ];
        let verified = original_preserved
            && upstream_private
            && private.iter().all(|private| !captured.contains(private))
            && captured.contains("jamendo_provider_complete")
            && captured.contains("request_id=");
        Report {
            outcome,
            connections,
            verified,
            jobs_settled,
            database_retired: retirement.resources_retired,
            database_deadline_exceeded: retirement.deadline_exceeded,
        }
    }
}

fn merge(total: &mut Shutdown, report: Shutdown) {
    total.accepted += report.accepted;
    total.joined += report.joined;
    total.aborted += report.aborted;
    total.timed_out |= report.timed_out;
    total.failed |= report.failed;
}

async fn settle(
    services: Option<Services>,
    owner: Option<DatabaseOwner>,
    deadline: Option<Duration>,
) -> (bool, hito_core::test_support::database_owner::Retirement) {
    let jobs_settled = if let Some(services) = services {
        let stopping = services.downloads.finish_downloads();
        tokio::pin!(stopping);
        match tokio::time::timeout(Duration::from_secs(5), &mut stopping).await {
            Ok(result) => result.is_ok(),
            Err(_) => {
                let _ = stopping.await;
                false
            }
        }
    } else {
        true
    };
    let retirement = match owner {
        Some(owner) => {
            owner
                .retire(deadline.unwrap_or(Duration::from_secs(5)))
                .await
        }
        None => hito_core::test_support::database_owner::Retirement {
            deadline_exceeded: false,
            resources_retired: true,
        },
    };
    (jobs_settled, retirement)
}

pub(super) struct Run {
    stop: Option<oneshot::Sender<()>>,
    finished: oneshot::Receiver<()>,
    thread: Option<std::thread::JoinHandle<Report>>,
    observation: Observation,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Run {
    pub(super) fn start(
        check: impl AsyncFnOnce(&mut Host) + Send + 'static,
        gate: Option<Arc<StartupGate>>,
        observation: Observation,
    ) -> Self {
        let (stop, mut cancelled) = oneshot::channel();
        let (finished, settled) = oneshot::channel();
        let observed = observation.clone();
        let thread = std::thread::spawn(move || {
            let directory = Arc::new(
                tempfile::tempdir().unwrap_or_else(|_| panic!("owned host directory failed")),
            );
            *observed.directory.lock().unwrap() = Some(directory.path().to_path_buf());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|_| panic!("owned fixture runtime failed"));
            let report = runtime.block_on(async {
                let mut startup = Startup::new(directory.clone(), observed.clone());
                let initialized = tokio::select! {
                    biased;
                    _ = &mut cancelled => Err(Outcome::Cancelled),
                    result = AssertUnwindSafe(startup.initialize(gate)).catch_unwind() =>
                        result.map_err(|_| Outcome::Panicked),
                };
                if let Err(outcome) = initialized { return startup.finish(outcome).await; }
                let mut host = startup.complete();
                let outcome = tokio::select! {
                    biased;
                    _ = &mut cancelled => Outcome::Cancelled,
                    result = tokio::time::timeout(Duration::from_secs(30), AssertUnwindSafe(check(&mut host)).catch_unwind()) => match result {
                        Ok(Ok(())) => Outcome::Complete,
                        Ok(Err(_)) => Outcome::Panicked,
                        Err(_) => Outcome::Deadline,
                    },
                };
                host.finish(outcome).await
            });
            drop(runtime);
            drop(directory);
            *observed.report.lock().unwrap() = Some(report);
            let _ = finished.send(());
            report
        });
        Self {
            stop: Some(stop),
            finished: settled,
            thread: Some(thread),
            observation,
            _not_send: std::marker::PhantomData,
        }
    }

    pub(super) fn join(&mut self) -> Report {
        let report = self
            .thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or(Report {
                outcome: Outcome::Panicked,
                connections: Shutdown {
                    failed: true,
                    ..Default::default()
                },
                verified: false,
                jobs_settled: false,
                database_retired: false,
                database_deadline_exceeded: false,
            });
        *self.observation.report.lock().unwrap() = Some(report);
        report
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if self.thread.is_some() {
            let _ = self.join();
        }
    }
}

pub(super) async fn observed_host(
    check: impl AsyncFnOnce(&mut Host) + Send + 'static,
    gate: Option<Arc<StartupGate>>,
    observation: Observation,
) -> Report {
    let mut run = Run::start(check, gate, observation);
    let _ = (&mut run.finished).await;
    run.join()
}

pub(super) async fn with_host(check: impl AsyncFnOnce(&mut Host) + Send + 'static) {
    let report = observed_host(check, None, Observation::default()).await;
    assert!(
        report.outcome == Outcome::Complete,
        "owned fixture workflow did not complete"
    );
    assert!(
        report.connections.clean()
            && report.jobs_settled
            && report.database_retired
            && !report.database_deadline_exceeded,
        "owned fixture cleanup settled with a failure"
    );
    assert!(
        report.verified,
        "owned fixture privacy or original preservation failed"
    );
}

pub(super) struct RequestHold {
    pub(super) entered: Semaphore,
    release: Semaphore,
    pub(super) active: std::sync::atomic::AtomicUsize,
    pub(super) early_storage_release: std::sync::atomic::AtomicBool,
}

impl RequestHold {
    pub(super) fn new() -> Self {
        Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            active: std::sync::atomic::AtomicUsize::new(0),
            early_storage_release: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

struct HeldCore {
    core: Services,
    directory: PathBuf,
    hold: Arc<RequestHold>,
}

impl Drop for HeldCore {
    fn drop(&mut self) {
        let _ = &self.core;
        if !self.directory.exists() {
            self.hold
                .early_storage_release
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.hold
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn open(directory: &Path, owner: &DatabaseOwner) -> Services {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("owned-jamendo-host").unwrap(),
    );
    owner.attach(&mut config);
    hito_core::open(config)
        .await
        .unwrap_or_else(|_| panic!("temporary SQLite host failed"))
}

fn database(directory: Arc<tempfile::TempDir>, observation: &Observation) -> DatabaseOwner {
    let owner = DatabaseOwner::retain(directory);
    if let Some(gate) = &observation.database_opening {
        owner.hold_opening(gate);
    }
    if let Some(gate) = &observation.database_retirement {
        owner.hold_retirement(gate);
    }
    *observation.database.lock().unwrap() = Some(owner.observe());
    observation.database_ready.add_permits(1);
    owner
}
