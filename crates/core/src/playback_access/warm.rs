use super::{AudioRequest, OpenedAudio, PlaybackSource};
use crate::{Actor, Error, Result};
use bytes::Bytes;
use futures_util::{FutureExt, future::{BoxFuture, Shared}};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak, atomic::{AtomicBool, Ordering}},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Notify, watch};
use uuid::Uuid;

const ENTRIES_PER_ACCOUNT: usize = 1;
const ENTRIES_GLOBAL: usize = 2;
const PREFIX_MAX: usize = 65_536;
const RESERVATION: usize = 131_072;
const START_DEADLINE: Duration = Duration::from_secs(2);
const HOLD: Duration = Duration::from_secs(60);

type Completion = Shared<BoxFuture<'static, std::result::Result<(), Arc<Error>>>>;

struct Ready {
    content: Box<crate::AudioContent>,
    prefix: Option<Bytes>,
}

enum Command {
    Read(tokio::sync::oneshot::Sender<Result<Option<Bytes>>>),
    Close(tokio::sync::oneshot::Sender<()>),
}

pub(crate) struct WarmBody {
    commands: tokio::sync::mpsc::Sender<Command>,
    grant: PlaybackSource,
}

impl WarmBody {
    pub(crate) async fn check_prefix(&self) -> Result<()> {
        self.grant.warm_check().await
    }

    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>> {
        let (reply, received) = tokio::sync::oneshot::channel();
        self.commands.send(Command::Read(reply)).await.map_err(|_| Error::NotFound)?;
        received.await.map_err(|_| Error::NotFound)?
    }

    pub(crate) async fn shutdown(&mut self) {
        let (reply, received) = tokio::sync::oneshot::channel();
        if self.commands.send(Command::Close(reply)).await.is_ok() {
            let _ = received.await;
        }
    }
}

impl Drop for WarmBody {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Close(tokio::sync::oneshot::channel().0));
    }
}

struct Entry {
    lease: Uuid,
    actor: Actor,
    grant: PlaybackSource,
    started: std::time::Instant,
    start_deadline: tokio::time::Instant,
    deadline: tokio::time::Instant,
    canceled: watch::Sender<bool>,
    claimed: watch::Sender<bool>,
    ready: AsyncMutex<Option<Ready>>,
    changed: Notify,
    registry: Weak<Registry>,
    reservation: Arc<Reservation>,
    ticket: Mutex<Option<Arc<crate::plugins::WarmTicket>>>,
    commands: tokio::sync::mpsc::Sender<Command>,
}

impl Entry {
    fn stop(&self) {
        self.canceled.send_replace(true);
        self.changed.notify_one();
    }
}

#[derive(Default)]
struct State {
    closed: bool,
    entries: HashMap<Uuid, Arc<Entry>>,
    completions: HashMap<Uuid, Work>,
    reserved_bytes: usize,
}

struct Work {
    finished: tokio::task::AbortHandle,
    completion: Completion,
}

#[derive(Default)]
pub(crate) struct Registry {
    state: Mutex<State>,
}

pub(crate) struct Taken {
    pub content: Box<crate::AudioContent>,
    pub prefix: Option<Bytes>,
    pub body: WarmBody,
    pub reservation: Arc<Reservation>,
}

impl Registry {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    pub(crate) async fn try_start(
        self: &Arc<Self>,
        lease: Uuid,
        actor: &Actor,
        lease_cancel: watch::Receiver<bool>,
        grant: PlaybackSource,
        lease_deadline: tokio::time::Instant,
    ) {
        if !grant.is_provider_remote() {
            return;
        }
        let now = tokio::time::Instant::now();
        let start_deadline = (now + START_DEADLINE).min(lease_deadline);
        let deadline = (now + HOLD).min(lease_deadline);
        let (canceled, cancel_rx) = watch::channel(false);
        let (claimed, claim_rx) = watch::channel(false);
        let (commands, command_rx) = tokio::sync::mpsc::channel(2);
        let entry = Arc::new(Entry {
            lease,
            actor: actor.clone(),
            grant: grant.clone(),
            started: std::time::Instant::now(),
            start_deadline,
            deadline,
            canceled,
            claimed,
            ready: AsyncMutex::new(None),
            changed: Notify::new(),
            registry: Arc::downgrade(self),
            reservation: Arc::new(Reservation {
                registry: Arc::downgrade(self),
                lease,
                bytes: RESERVATION,
                accounted: AtomicBool::new(false),
            }),
            ticket: Mutex::new(None),
            commands: commands.clone(),
        });
        {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let count = state
                .entries
                .values()
                .filter(|existing| existing.actor.id() == actor.id())
                .count();
            if state.closed
                || count >= ENTRIES_PER_ACCOUNT
                || state.entries.len() >= ENTRIES_GLOBAL
                || state.reserved_bytes + RESERVATION > ENTRIES_GLOBAL * RESERVATION
            {
                return;
            }
            state.reserved_bytes += RESERVATION;
            entry.reservation.accounted.store(true, Ordering::Release);
            state.entries.insert(lease, entry.clone());
        }
        tracing::debug!(
            event = "provider_audio_warm_started",
            outcome = "reserved",
            reserved_bytes = RESERVATION,
            "Provider audio warm work reserved"
        );
        let worker_entry = entry.clone();
        let task = tokio::spawn(async move {
            let mut lease_cancel = lease_cancel;
            let mut cancel_rx = cancel_rx;
            let opened = tokio::select! {
                biased;
                _ = lease_cancel.changed() => None,
                _ = cancel_rx.changed() => None,
                _ = tokio::time::sleep_until(start_deadline) => None,
                result = tokio::time::timeout_at(start_deadline, async {
                    grant.warm_check().await?;
                    grant.warm_open(AudioRequest::default()).await
                }) => result.ok().and_then(Result::ok),
            };
            if let Some((OpenedAudio::Content(mut content), ticket)) = opened {
                if let Ok(mut current) = worker_entry.ticket.lock() {
                    *current = Some(ticket.clone());
                }
                let mut ticket_cancel = ticket.cancelled();
                if let Some(mut body) = content.body.take() {
                    *worker_entry.ready.lock().await = Some(Ready {
                        content,
                        prefix: None,
                    });
                    worker_entry.changed.notify_one();
                    let mut first_read = Some(Box::pin(body.chunk()));
                    let mut first_result = None;
                    let mut waiting_reader = None;
                    let mut claimed = ticket.is_claimed() || *claim_rx.borrow();
                    let mut first_ended = false;
                    while first_read.is_some() {
                        tokio::select! {
                            biased;
                            _ = lease_cancel.changed() => break,
                            _ = cancel_rx.changed() => break,
                            _ = ticket_cancel.changed() => break,
                            _ = tokio::time::sleep_until(deadline), if !claimed => break,
                            _ = tokio::time::sleep_until(start_deadline), if !claimed => break,
                            _ = claim_rx.changed(), if !claimed => claimed = true,
                            command = command_rx.recv() => match command {
                                Some(Command::Read(reply)) if waiting_reader.is_none() => waiting_reader = Some(reply),
                                Some(Command::Read(reply)) => {
                                    let _ = reply.send(Err(Error::Conflict("Audio body is busy. Try again.".into())));
                                }
                                Some(Command::Close(reply)) => {
                                    let _ = reply.send(());
                                    break;
                                }
                                None => break,
                            },
                            result = async {
                                first_read.as_mut().expect("pending warm read").as_mut().await
                            } => {
                                first_read.take();
                                claimed |= ticket.is_claimed() || *claim_rx.borrow();
                                let result = if !claimed && matches!(&result, Ok(Some(_))) {
                                    match grant.warm_check().await {
                                        Ok(()) => result,
                                        Err(error) => Err(error),
                                    }
                                } else {
                                    result
                                };
                                let valid_prefix = matches!(&result, Ok(Some(bytes)) if !bytes.is_empty() && bytes.len() <= PREFIX_MAX);
                                if !valid_prefix && !claimed {
                                    break;
                                }
                                if let Some(reply) = waiting_reader.take() {
                                    first_ended = !matches!(&result, Ok(Some(_)));
                                    let _ = reply.send(result);
                                } else if valid_prefix && !claimed {
                                    let mut stored = false;
                                    let mut ready = worker_entry.ready.lock().await;
                                    if !ticket.is_claimed()
                                        && let Some(ready) = ready.as_mut()
                                        && let Ok(Some(prefix)) = &result
                                    {
                                        let mut buffer = Vec::with_capacity(PREFIX_MAX);
                                        buffer.extend_from_slice(prefix);
                                        ready.prefix = Some(Bytes::from(buffer));
                                        stored = true;
                                        worker_entry.changed.notify_one();
                                        tracing::debug!(
                                            event = "provider_audio_warm_prefix_ready",
                                            outcome = "ready",
                                            prefix_bytes = prefix.len(),
                                            elapsed_ms = worker_entry.started.elapsed().as_millis() as u64,
                                            "Provider audio warm prefix ready"
                                        );
                                    }
                                    drop(ready);
                                    if !stored {
                                        first_result = Some(result);
                                    }
                                } else {
                                    first_result = Some(result);
                                }
                            }
                        }
                    }
                    if !claimed && !ticket.is_claimed() && !*claim_rx.borrow() {
                        let retained_prefix = worker_entry
                            .ready
                            .lock()
                            .await
                            .as_ref()
                            .is_some_and(|ready| ready.prefix.is_some());
                        if first_result.is_some() || retained_prefix {
                            tokio::select! {
                                biased;
                                _ = lease_cancel.changed() => {}
                                _ = cancel_rx.changed() => {}
                                _ = ticket_cancel.changed() => {}
                                _ = tokio::time::sleep_until(deadline) => {}
                                _ = claim_rx.changed() => claimed = true,
                            }
                        }
                    }
                    claimed |= ticket.is_claimed() || *claim_rx.borrow();
                    if claimed && !first_ended {
                        loop {
                            let command = tokio::select! {
                                biased;
                                _ = lease_cancel.changed() => break,
                                _ = cancel_rx.changed() => break,
                                _ = ticket_cancel.changed() => break,
                                command = command_rx.recv() => command,
                            };
                            match command {
                                Some(Command::Read(reply)) => {
                                    let result = if let Some(result) = first_result.take() {
                                        result
                                    } else {
                                        tokio::select! {
                                            biased;
                                            _ = lease_cancel.changed() => Err(Error::NotFound),
                                            _ = cancel_rx.changed() => Err(Error::NotFound),
                                            _ = ticket_cancel.changed() => Err(Error::NotFound),
                                            command = command_rx.recv() => match command {
                                                Some(Command::Close(close)) => {
                                                    let _ = close.send(());
                                                    Err(Error::NotFound)
                                                }
                                                Some(Command::Read(extra)) => {
                                                    let _ = extra.send(Err(Error::Conflict("Audio body is busy. Try again.".into())));
                                                    Err(Error::NotFound)
                                                }
                                                None => Err(Error::NotFound),
                                            },
                                            result = body.chunk() => result,
                                        }
                                    };
                                    let ended = !matches!(&result, Ok(Some(_)));
                                    let _ = reply.send(result);
                                    if ended {
                                        break;
                                    }
                                }
                                Some(Command::Close(reply)) => {
                                    let _ = reply.send(());
                                    break;
                                }
                                None => break,
                            }
                        }
                    }
                    drop(first_read);
                    body.shutdown().await;
                }
            }
            if let Some(registry) = worker_entry.registry.upgrade()
                && let Ok(mut state) = registry.state.lock()
                && state
                    .entries
                    .get(&worker_entry.lease)
                    .is_some_and(|current| Arc::ptr_eq(current, &worker_entry))
            {
                state.entries.remove(&worker_entry.lease);
            }
            worker_entry.stop();
            if let Ok(mut ticket) = worker_entry.ticket.lock() {
                ticket.take();
            }
            *worker_entry.ready.lock().await = None;
            worker_entry.changed.notify_one();
            tracing::debug!(
                event = "provider_audio_warm_retired",
                outcome = "retired",
                elapsed_ms = worker_entry.started.elapsed().as_millis() as u64,
                "Provider audio warm work retired"
            );
        });
        let finished = task.abort_handle();
        let completion: Completion = async move { task.await.map_err(|_| Arc::new(Error::NotFound))? }
            .boxed()
            .shared();
        if let Ok(mut state) = self.state.lock() {
            state.completions.retain(|_, work| !work.finished.is_finished());
            state.completions.insert(
                lease,
                Work {
                    finished,
                    completion,
                },
            );
        }
    }

    pub(crate) async fn claim(
        &self,
        actor: &Actor,
        lease: Uuid,
        grant: &PlaybackSource,
        request: AudioRequest,
    ) -> Result<Option<Taken>> {
        if request.head || request.range.is_some() {
            self.retire_and_join(lease).await?;
            return Ok(None);
        }
        let entry = {
            let state = self.state.lock().map_err(|_| Error::NotFound)?;
            state
                .entries
                .get(&lease)
                .filter(|entry| actor.same_session(&entry.actor))
                .cloned()
        };
        let Some(entry) = entry else {
            return Ok(None);
        };
        if !grant.warm_equivalent(&entry.grant) {
            self.retire_and_join(lease).await?;
            return Ok(None);
        }
        let wait = async {
            loop {
                let changed = entry.changed.notified();
                tokio::pin!(changed);
                grant.warm_check().await?;
                if let Some(ready) = entry.ready.lock().await.take() {
                    grant.warm_check().await?;
                    return Ok::<_, Error>(Some(ready));
                }
                if *entry.canceled.borrow() || tokio::time::Instant::now() >= entry.deadline {
                    return Ok(None);
                }
                changed.await;
            }
        };
        let wait_deadline = if entry.ready.lock().await.is_some() {
            entry.deadline
        } else {
            entry.start_deadline
        };
        let ready = match tokio::time::timeout_at(wait_deadline, wait).await {
            Ok(Ok(ready)) => ready,
            Ok(Err(error)) => {
                self.retire_and_join(lease).await?;
                return Err(error);
            }
            Err(_) => None,
        };
        let Some(ready) = ready else {
            self.retire_and_join(lease).await?;
            return Ok(None);
        };
        let ticket = entry
            .ticket
            .lock()
            .ok()
            .and_then(|ticket| ticket.as_ref().cloned());
        let claimed = {
            let mut state = self.state.lock().map_err(|_| Error::NotFound)?;
            let current = state
                .entries
                .get(&lease)
                .is_some_and(|current| Arc::ptr_eq(current, &entry));
            if current && ticket.as_ref().is_some_and(|ticket| ticket.claim()) {
                state.entries.remove(&lease);
                true
            } else {
                false
            }
        };
        if !claimed {
            drop(ready);
            self.retire_and_join(lease).await?;
            return Ok(None);
        }
        entry.claimed.send_replace(true);
        Ok(Some(Taken {
            content: ready.content,
            prefix: ready.prefix,
            body: WarmBody {
                commands: entry.commands.clone(),
                grant: entry.grant.clone(),
            },
            reservation: entry.reservation.clone(),
        }))
    }

    pub(crate) async fn retire_and_join(&self, lease: Uuid) -> Result<()> {
        let (entry, completion) = {
            let mut state = self.state.lock().map_err(|_| Error::NotFound)?;
            (
                state.entries.remove(&lease),
                state
                    .completions
                    .get(&lease)
                    .map(|work| work.completion.clone()),
            )
        };
        if let Some(entry) = entry {
            entry.stop();
        }
        if let Some(completion) = completion {
            completion.await.map_err(|_| Error::NotFound)?;
            if let Ok(mut state) = self.state.lock() {
                state.completions.remove(&lease);
            }
        }
        Ok(())
    }

    pub(crate) async fn close(&self) -> Result<()> {
        let (entries, completions) = {
            let mut state = self.state.lock().map_err(|_| Error::NotFound)?;
            state.closed = true;
            let entries = state.entries.drain().map(|(_, entry)| entry).collect::<Vec<_>>();
            let completions = state
                .completions
                .drain()
                .map(|(_, work)| work.completion)
                .collect::<Vec<_>>();
            for entry in &entries {
                entry.stop();
            }
            (entries, completions)
        };
        drop(entries);
        for completion in completions {
            completion.await.map_err(|_| Error::NotFound)?;
        }
        Ok(())
    }
}

pub(crate) struct Reservation {
    registry: Weak<Registry>,
    lease: Uuid,
    bytes: usize,
    accounted: AtomicBool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.accounted.swap(false, Ordering::AcqRel)
            && let Some(registry) = self.registry.upgrade()
            && let Ok(mut state) = registry.state.lock()
        {
            state.reserved_bytes = state.reserved_bytes.saturating_sub(self.bytes);
        }
        let _ = self.lease;
    }
}
