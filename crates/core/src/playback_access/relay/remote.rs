use super::pace::{Meter, StreamPace};
use crate::{Error, Result};
use bytes::Bytes;
use futures_util::{FutureExt, future::{BoxFuture, Shared}};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex as StdMutex, Weak, atomic::{AtomicU64, Ordering}},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, watch};
use tracing::{Instrument, instrument::WithSubscriber};

type MonitorCompletion = Shared<BoxFuture<'static, std::result::Result<(), Arc<Error>>>>;

#[derive(Default)]
struct MonitorState {
    closed: bool,
    work: HashMap<u64, MonitorWork>,
}

struct MonitorWork {
    stop: watch::Sender<bool>,
    finished: tokio::task::AbortHandle,
    completion: MonitorCompletion,
}

#[derive(Default)]
pub(crate) struct MonitorRegistry {
    state: StdMutex<MonitorState>,
    next: AtomicU64,
}

impl MonitorRegistry {
    fn spawn(
        &self,
        stop: watch::Sender<bool>,
        monitor: impl Future<Output = ()> + Send + 'static,
    ) -> Result<u64> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Err(Error::NotFound);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let task = tokio::spawn(monitor);
        let finished = task.abort_handle();
        let completion = async move {
            task.await.map_err(|_| Arc::new(Error::NotFound))?;
            Ok(())
        }
        .boxed()
        .shared();
        state.work.retain(|_, work| !work.finished.is_finished());
        state.work.insert(id, MonitorWork { stop, finished, completion });
        Ok(id)
    }

    fn stop(&self, id: u64) {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(work) = state.work.get(&id) {
            work.stop.send_replace(true);
        }
    }

    async fn stop_and_join(&self, id: u64) -> Result<()> {
        let completion = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.work.get(&id).map(|work| {
                work.stop.send_replace(true);
                work.completion.clone()
            })
        };
        if let Some(completion) = completion {
            completion.await.map_err(|_| Error::NotFound)?;
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.work.remove(&id);
        }
        Ok(())
    }

    pub(crate) async fn close(&self) -> Result<()> {
        let completions = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.closed = true;
            state.work.values().map(|work| {
                work.stop.send_replace(true);
                work.completion.clone()
            }).collect::<Vec<_>>()
        };
        for completion in completions {
            completion.await.map_err(|_| Error::NotFound)?;
        }
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.work.retain(|_, work| !work.finished.is_finished());
        Ok(())
    }
}

pub(crate) trait AuthorityCheck: Send + Sync {
    fn check(&self) -> BoxFuture<'_, Result<()>>;
}

#[derive(Clone)]
pub(crate) struct Authority(Arc<dyn AuthorityCheck>);

impl Authority {
    pub(crate) fn new(authority: impl AuthorityCheck + 'static) -> Self {
        Self(Arc::new(authority))
    }

    async fn check(&self) -> Result<()> {
        self.0.check().await
    }
}

pub(crate) struct Permits {
    pub _account: OwnedSemaphorePermit,
    pub _global: OwnedSemaphorePermit,
    pub _warm: Option<Arc<crate::plugins::WarmTicket>>,
}

struct Connection {
    response: Option<reqwest::Response>,
    permits: Option<Permits>,
    pending: Bytes,
}

async fn release_connection(connection: &Weak<AsyncMutex<Connection>>) {
    if let Some(connection) = connection.upgrade() {
        let mut connection = connection.lock().await;
        connection.response = None;
        connection.permits = None;
        connection.pending = Bytes::new();
    }
}

pub(crate) struct RemoteAudio {
    connection: Arc<AsyncMutex<Connection>>,
    authority: Authority,
    revoked: watch::Receiver<bool>,
    monitor_stop: watch::Sender<bool>,
    warm_monitor: Option<(Arc<MonitorRegistry>, u64)>,
    monitor_task: Option<tokio::task::JoinHandle<()>>,
    meter: Meter,
}
impl RemoteAudio {
    pub(crate) fn new(
        response: reqwest::Response,
        authority: Authority,
        permits: Option<Permits>,
        pace: StreamPace,
        warm_monitors: Option<Arc<MonitorRegistry>>,
    ) -> Result<Self> {
        let connection = Arc::new(AsyncMutex::new(Connection {
            response: Some(response),
            permits,
            pending: Bytes::new(),
        }));
        let weak = Arc::downgrade(&connection);
        let (cancel, revoked) = watch::channel(false);
        let monitor_cancel = cancel.clone();
        let guard = authority.clone();
        let mut stop = cancel.subscribe();
        let monitor = async move {
                loop {
                    let stopping = tokio::select! {
                        biased;
                        _ = stop.changed() => true,
                        _ = tokio::time::sleep(Duration::from_millis(250)) => {
                            let checked = tokio::select! {
                                biased;
                                _ = stop.changed() => None,
                                checked = guard.check() => Some(checked),
                            };
                            match checked {
                                Some(Ok(())) => false,
                                Some(Err(_)) => {
                                    let _ = monitor_cancel.send(true);
                                    release_connection(&weak).await;
                                    tracing::debug!(
                                        event = "remote_audio_authority_withdrawn",
                                        "Remote audio authority withdrawn"
                                    );
                                    true
                                }
                                None => true,
                            }
                        }
                    };
                    if stopping {
                        release_connection(&weak).await;
                        break;
                    }
                }
            }
            .instrument(tracing::Span::current())
            .with_current_subscriber();
        let (warm_monitor, monitor_task) = if let Some(monitors) = warm_monitors {
            (Some((monitors.clone(), monitors.spawn(cancel.clone(), monitor)?)), None)
        } else {
            (None, Some(tokio::spawn(monitor)))
        };
        Ok(Self {
            connection,
            authority,
            revoked,
            monitor_stop: cancel,
            warm_monitor,
            monitor_task,
            meter: Meter::new(pace),
        })
    }
    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>> {
        self.authority.check().await?;
        let mut connection = self.connection.lock().await;
        if *self.revoked.borrow() {
            return Err(Error::NotFound);
        }
        let warm_read = connection
            .permits
            .as_ref()
            .and_then(|permits| permits._warm.as_ref())
            .is_some_and(|ticket| ticket.is_preparing());
        let result = if connection.pending.is_empty() {
            let response = connection.response.as_mut().ok_or(Error::NotFound)?;
            let waiting = Instant::now();
            let read = tokio::select! {
                biased;
                _ = self.revoked.changed() => Err(Error::NotFound),
                result = tokio::time::timeout(self.meter.read_idle(), response.chunk()) => {
                    match result {
                        Ok(chunk) => chunk.map_err(|_| Error::Upstream("The audio stream could not be read")),
                        Err(_) => Err(Error::Upstream("The audio stream did not return data in time")),
                    }
                },
            };
            match read {
                Ok(Some(bytes))
                    if bytes.len() > 65536 && warm_read =>
                {
                    Err(Error::Upstream("The audio service returned an oversized chunk"))
                }
                Ok(Some(bytes)) => self
                    .meter
                    .record(waiting.elapsed(), bytes.len())
                    .map(|()| Some(bytes)),
                other => other,
            }
        } else {
            Ok(Some(std::mem::take(&mut connection.pending)))
        };
        self.authority.check().await?;
        let result = result.map(|chunk| {
            chunk.map(|mut bytes| {
                if bytes.len() > 65536 {
                    connection.pending = bytes.split_off(65536);
                }
                bytes
            })
        });
        if matches!(&result, Ok(None)) {
            drop(connection);
            self.shutdown().await;
        }
        result
    }

    pub(crate) async fn shutdown(&mut self) {
        self.monitor_stop.send_replace(true);
        {
            let mut connection = self.connection.lock().await;
            connection.response = None;
            connection.permits = None;
            connection.pending = Bytes::new();
        }
        if let Some((monitors, monitor_id)) = &self.warm_monitor {
            let _ = monitors.stop_and_join(*monitor_id).await;
        }
        if let Some(task) = self.monitor_task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}
impl Drop for RemoteAudio {
    fn drop(&mut self) {
        self.monitor_stop.send_replace(true);
        if let Some((monitors, monitor_id)) = &self.warm_monitor {
            monitors.stop(*monitor_id);
        }
        if let Some(task) = self.monitor_task.take() {
            task.abort();
        }
    }
}
