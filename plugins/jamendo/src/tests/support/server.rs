use axum::{Router, routing::get};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use tokio::{
    net::TcpListener,
    sync::{oneshot, watch},
    task::JoinSet,
};
use tracing::instrument::WithSubscriber;
use url::Url;

#[derive(Clone, Copy, Default)]
pub struct Shutdown {
    pub accepted: usize,
    pub joined: usize,
    pub aborted: usize,
    pub timed_out: bool,
    pub failed: bool,
}

impl Shutdown {
    pub fn clean(self) -> bool {
        !self.failed && !self.timed_out && self.accepted == self.joined
    }
}

struct Owner {
    stop: Option<oneshot::Sender<()>>,
    finished: oneshot::Receiver<()>,
    thread: Option<std::thread::JoinHandle<Shutdown>>,
}

impl Owner {
    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    fn join(&mut self) -> Shutdown {
        self.stop();
        self.thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or(Shutdown {
                failed: true,
                ..Default::default()
            })
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let report = self.join();
            if !std::thread::panicking() {
                assert!(
                    report.clean(),
                    "owned server drop settled with a shutdown failure"
                );
            }
        }
    }
}

pub struct Server {
    pub base: Url,
    owner: Owner,
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Server {
    pub async fn start(router: Router) -> Self {
        Self::build(move |_| (router, ())).await.0
    }

    pub async fn start_with_dispatch(router: Router, dispatch: tracing::Dispatch) -> Self {
        let router = router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let dispatch = dispatch.clone();
                async move { next.run(request).with_subscriber(dispatch).await }
            },
        ));
        Self::start(router).await
    }

    pub async fn build<T: Send + 'static>(
        make: impl FnOnce(Url) -> (Router, T) + Send + 'static,
    ) -> (Self, T) {
        Self::build_before_ready(make, None).await
    }

    async fn build_before_ready<T: Send + 'static>(
        make: impl FnOnce(Url) -> (Router, T) + Send + 'static,
        before_ready: Option<Arc<Semaphore>>,
    ) -> (Self, T) {
        let (ready, address) = oneshot::channel();
        let (stop, stopped) = oneshot::channel();
        let (finished, settled) = oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|_| panic!("owned server runtime failed"));
            let result = runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .unwrap_or_else(|_| panic!("owned server listener failed"));
                let address = listener
                    .local_addr()
                    .unwrap_or_else(|_| panic!("owned server address failed"));
                let base = Url::parse(&format!("http://{address}/"))
                    .unwrap_or_else(|_| panic!("owned server origin failed"));
                let (router, state) = make(base.clone());
                let mut ready = ready;
                if let Some(before_ready) = before_ready {
                    before_ready.add_permits(1);
                    ready.closed().await;
                }
                if ready.send((base, state)).is_err() {
                    return Shutdown::default();
                }
                serve(listener, router, stopped).await
            });
            drop(runtime);
            let _ = finished.send(());
            result
        });
        let owner = Owner {
            stop: Some(stop),
            finished: settled,
            thread: Some(thread),
        };
        let (base, state) = address
            .await
            .unwrap_or_else(|_| panic!("owned server startup failed"));
        (
            Self {
                base,
                owner,
                _not_send: std::marker::PhantomData,
            },
            state,
        )
    }

    pub async fn shutdown(self) -> bool {
        self.shutdown_report().await.clean()
    }

    pub async fn shutdown_report(mut self) -> Shutdown {
        self.owner.stop();
        let _ = (&mut self.owner.finished).await;
        self.owner.join()
    }
}

async fn serve(
    listener: TcpListener,
    router: Router,
    mut stopped: oneshot::Receiver<()>,
) -> Shutdown {
    let (stop_connections, stopping) = watch::channel(false);
    let mut connections = JoinSet::new();
    let mut report = Shutdown::default();
    loop {
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            joined = connections.join_next(), if !connections.is_empty() => {
                report.joined += 1;
                report.failed |= !matches!(joined, Some(Ok(true)));
            }
            accepted = listener.accept() => {
                let Ok((socket, _)) = accepted else { report.failed = true; break; };
                report.accepted += 1;
                let router = router.clone();
                let mut stopping = stopping.clone();
                connections.spawn(async move {
                    let connection = http1::Builder::new().serve_connection(
                        TokioIo::new(socket), TowerToHyperService::new(router),
                    );
                    tokio::pin!(connection);
                    tokio::select! {
                        result = &mut connection => result.is_ok(),
                        _ = stopping.changed() => {
                            connection.as_mut().graceful_shutdown();
                            connection.await.is_ok()
                        }
                    }
                });
            }
        }
    }
    drop(listener);
    let _ = stop_connections.send(true);
    let graceful = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(joined) = connections.join_next().await {
            report.joined += 1;
            report.failed |= !matches!(joined, Ok(true));
        }
    })
    .await;
    if graceful.is_err() {
        report.timed_out = true;
        report.aborted = connections.len();
        connections.abort_all();
        while connections.join_next().await.is_some() {
            report.joined += 1;
        }
    }
    report
}

struct Active(Arc<AtomicUsize>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn cancelling_before_readiness_settles_without_panicking_in_a_disposable_child() {
    let (status, output) = disposable_child("contract::server::shutdown_child", "ready_cancel");
    assert!(
        output.contains("owned_startup_control stage=before_drop"),
        "owned startup control missed its boundary"
    );
    assert!(
        status.success(),
        "expected startup cancellation panicked after joining its worker"
    );
}

#[test]
fn shutdown_deadline_joins_a_held_handler_in_a_disposable_child() {
    shutdown_child_process("handler");
}

#[test]
fn shutdown_deadline_joins_a_held_body_in_a_disposable_child() {
    shutdown_child_process("body");
}

fn shutdown_child_process(kind: &str) {
    let (status, output) = disposable_child("contract::server::shutdown_child", kind);
    assert!(
        output.contains("owned_shutdown_control stage=shutdown_returned"),
        "owned child failed before the shutdown boundary"
    );
    assert!(
        status.success(),
        "held connection remained alive after server shutdown returned"
    );
}

struct ChildOwner(std::process::Child);

impl Drop for ChildOwner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn disposable_child(test: &str, kind: &str) -> (std::process::ExitStatus, String) {
    let executable =
        std::env::current_exe().unwrap_or_else(|_| panic!("owned control executable unavailable"));
    let mut child = ChildOwner(
        Command::new(executable)
            .args(["--exact", test, "--nocapture"])
            .env("HITO_JAMENDO_OWNED_CHILD", kind)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|_| panic!("owned control child did not start")),
    );
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .0
            .try_wait()
            .unwrap_or_else(|_| panic!("owned control child status failed"))
        {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "owned control child exceeded its kill and join boundary"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut output = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(8192)
        .read_to_string(&mut output)
        .unwrap_or_else(|_| panic!("owned child stdout failed"));
    child
        .0
        .stderr
        .take()
        .unwrap()
        .take(8192)
        .read_to_string(&mut output)
        .unwrap_or_else(|_| panic!("owned child stderr failed"));
    (status, output)
}

#[tokio::test]
async fn shutdown_child() {
    let kind = std::env::var("HITO_JAMENDO_OWNED_CHILD").ok();
    let Some(kind) =
        kind.filter(|kind| matches!(kind.as_str(), "handler" | "body" | "ready_cancel"))
    else {
        return;
    };
    if kind == "ready_cancel" {
        let entered = Arc::new(Semaphore::new(0));
        let mut startup = Box::pin(Server::build_before_ready(
            |_| (Router::new(), ()),
            Some(entered.clone()),
        ));
        tokio::select! {
            permit = entered.acquire() => permit.unwrap_or_else(|_| panic!("owned startup gate closed")).forget(),
            _ = &mut startup => panic!("owned startup passed its readiness gate"),
            _ = tokio::time::sleep(Duration::from_secs(2)) => panic!("owned startup did not reach its readiness gate"),
        }
        println!("owned_startup_control stage=before_drop");
        let cancelled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(startup)));
        assert!(
            cancelled.is_ok(),
            "expected startup cancellation must not panic"
        );
        return;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let active = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Semaphore::new(0));
    let request_active = active.clone();
    let request_entered = entered.clone();
    let server = Server::start(Router::new().route(
        "/held",
        get(move || {
            let active = request_active.clone();
            let entered = request_entered.clone();
            let kind = kind.clone();
            async move {
                active.fetch_add(1, Ordering::SeqCst);
                let active = Active(active);
                if kind == "handler" {
                    let _active = active;
                    entered.add_permits(1);
                    std::future::pending::<()>().await;
                    axum::response::Response::new(axum::body::Body::empty())
                } else {
                    axum::response::Response::new(axum::body::Body::from_stream(HeldBody {
                        _active: active,
                        entered,
                    }))
                }
            }
        }),
    ))
    .await;
    let endpoint = server.base.join("held").unwrap();
    let request = tokio::spawn(async move {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| panic!("owned child client failed"))
            .get(endpoint)
            .send()
            .await
            .unwrap_or_else(|_| panic!("owned control request failed"))
            .bytes()
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.acquire())
        .await
        .ok()
        .and_then(Result::ok)
        .expect("owned held handler did not start")
        .forget();
    let report = server.shutdown_report().await;
    assert!(
        report.timed_out && !report.clean(),
        "held shutdown must preserve its deadline failure"
    );
    assert!(
        report.accepted == 1 && report.joined == 1 && report.aborted == 1,
        "all owned connection tasks must join"
    );
    println!("owned_shutdown_control stage=shutdown_returned");
    assert!(
        active.load(Ordering::SeqCst) == 0,
        "held handler survived shutdown settlement"
    );
    request.abort();
    let _ = request.await;
}

struct HeldBody {
    _active: Active,
    entered: Arc<Semaphore>,
}

impl futures_util::Stream for HeldBody {
    type Item = Result<axum::body::Bytes, std::io::Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.entered.add_permits(1);
        std::task::Poll::Pending
    }
}
