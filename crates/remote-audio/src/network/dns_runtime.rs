use super::{DnsPurpose, NetworkError};
use futures_util::{Stream, future::BoxFuture, stream::FuturesUnordered, task::AtomicWaker};
use hickory_resolver::net::{
    NetError,
    runtime::{RuntimeProvider, Spawn, TokioRuntimeProvider},
};
use std::{
    future::{Future, poll_fn},
    io,
    net::SocketAddr,
    pin::{Pin, pin},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
    time::{Duration, Instant},
};

const MAX_BACKGROUND_FUTURES: usize = 16;

struct Queue {
    pending: Mutex<Vec<BoxFuture<'static, ()>>>,
    spawned: AtomicUsize,
    failed: AtomicBool,
    closed: AtomicBool,
    waker: AtomicWaker,
}

#[derive(Clone)]
pub(super) struct InlineSpawn {
    queue: Weak<Queue>,
}

impl Spawn for InlineSpawn {
    fn spawn_bg(&mut self, future: impl Future<Output = ()> + Send + 'static) {
        let Some(queue) = self.queue.upgrade() else {
            return;
        };
        if queue.closed.load(Ordering::Acquire) {
            return;
        }
        let mut count = queue.spawned.load(Ordering::Acquire);
        let admitted = loop {
            if count >= MAX_BACKGROUND_FUTURES {
                break false;
            }
            match queue.spawned.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break true,
                Err(observed) => count = observed,
            }
        };
        if !admitted {
            queue.failed.store(true, Ordering::Release);
            queue.waker.wake();
            return;
        }
        match queue.pending.lock() {
            Ok(mut pending) => pending.push(Box::pin(future)),
            Err(_) => queue.failed.store(true, Ordering::Release),
        }
        queue.waker.wake();
    }
}

#[derive(Clone)]
pub(super) struct InlineRuntime {
    spawner: InlineSpawn,
    io: TokioRuntimeProvider,
}

impl RuntimeProvider for InlineRuntime {
    type Handle = InlineSpawn;
    type Timer = <TokioRuntimeProvider as RuntimeProvider>::Timer;
    type Udp = <TokioRuntimeProvider as RuntimeProvider>::Udp;
    type Tcp = <TokioRuntimeProvider as RuntimeProvider>::Tcp;

    fn create_handle(&self) -> Self::Handle {
        self.spawner.clone()
    }

    fn connect_tcp(
        &self,
        server_addr: SocketAddr,
        bind_addr: Option<SocketAddr>,
        timeout: Option<Duration>,
    ) -> Pin<Box<dyn Send + Future<Output = Result<Self::Tcp, io::Error>>>> {
        self.io.connect_tcp(server_addr, bind_addr, timeout)
    }

    fn bind_udp(
        &self,
        local_addr: SocketAddr,
        server_addr: SocketAddr,
    ) -> Pin<Box<dyn Send + Future<Output = Result<Self::Udp, io::Error>>>> {
        self.io.bind_udp(local_addr, server_addr)
    }
}

pub(super) struct DnsScope {
    queue: Arc<Queue>,
    active: FuturesUnordered<BoxFuture<'static, ()>>,
    started: Instant,
    span: tracing::Span,
    dispatch: tracing::Dispatch,
    outcome: &'static str,
    purpose: DnsPurpose,
}

impl DnsScope {
    pub(super) fn new(purpose: DnsPurpose) -> Self {
        Self {
            queue: Arc::new(Queue {
                pending: Mutex::new(Vec::new()),
                spawned: AtomicUsize::new(0),
                failed: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                waker: AtomicWaker::new(),
            }),
            active: FuturesUnordered::new(),
            started: Instant::now(),
            span: tracing::Span::current(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            outcome: "cancelled",
            purpose,
        }
    }

    pub(super) fn provider(&self) -> InlineRuntime {
        InlineRuntime {
            spawner: InlineSpawn {
                queue: Arc::downgrade(&self.queue),
            },
            io: TokioRuntimeProvider::default(),
        }
    }

    pub(super) async fn run<T>(
        &mut self,
        future: impl Future<Output = Result<T, NetError>>,
    ) -> Result<T, NetworkError> {
        let mut future = pin!(future);
        let result = poll_fn(|cx| {
            self.queue.waker.register(cx.waker());
            if self.queue.failed.load(Ordering::Acquire) {
                return Poll::Ready(Err(NetworkError::Transport));
            }
            let result = future.as_mut().poll(cx);
            if self.drain().is_err() {
                return Poll::Ready(Err(NetworkError::Transport));
            }
            for _ in 0..MAX_BACKGROUND_FUTURES {
                match Pin::new(&mut self.active).poll_next(cx) {
                    Poll::Ready(Some(())) => continue,
                    _ => break,
                }
            }
            match self.drain() {
                Ok(true) => cx.waker().wake_by_ref(),
                Ok(false) => {}
                Err(_) => return Poll::Ready(Err(NetworkError::Transport)),
            }
            if self.queue.failed.load(Ordering::Acquire) {
                return Poll::Ready(Err(NetworkError::Transport));
            }
            result.map(|result| result.map_err(|_| NetworkError::Transport))
        })
        .await;
        self.outcome = result
            .as_ref()
            .map(|_| "complete")
            .unwrap_or_else(|error| error.outcome());
        result
    }

    fn drain(&mut self) -> Result<bool, NetworkError> {
        let pending = {
            let mut pending = self
                .queue
                .pending
                .lock()
                .map_err(|_| NetworkError::Transport)?;
            std::mem::take(&mut *pending)
        };
        let added = !pending.is_empty();
        for future in pending {
            self.active.push(future);
        }
        Ok(added)
    }
}

impl Drop for DnsScope {
    fn drop(&mut self) {
        self.queue.closed.store(true, Ordering::Release);
        drop(self.queue.waker.take());
        self.active.clear();
        let pending = {
            let mut pending = self
                .queue
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *pending)
        };
        drop(pending);
        tracing::dispatcher::with_default(&self.dispatch, || {
            let _entered = self.span.enter();
            tracing::debug!(
                event = self.purpose.event(),
                outcome = self.outcome,
                count = self.queue.spawned.load(Ordering::Acquire) as u64,
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "Remote audio DNS scope finished"
            );
        });
    }
}
