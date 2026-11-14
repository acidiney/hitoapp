use super::{AudioRequest, OpenedAudio, PlaybackSource};
use crate::{Actor, AudioStream, Error, Result};
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
}

type FirstRead = BoxFuture<'static, (AudioStream, Option<Result<Option<Bytes>>>)>;

async fn read_first(
    mut body: AudioStream,
    mut lease_cancel: watch::Receiver<bool>,
    mut cancel: watch::Receiver<bool>,
    mut ticket_cancel: watch::Receiver<bool>,
) -> (AudioStream, Option<Result<Option<Bytes>>>) {
    if *lease_cancel.borrow() || *cancel.borrow() || *ticket_cancel.borrow() {
        body.shutdown().await;
        return (body, None);
    }
    let result = tokio::select! {
        biased;
        _ = lease_cancel.changed() => None,
        _ = cancel.changed() => None,
        _ = ticket_cancel.changed() => None,
        result = body.chunk() => Some(result),
    };
    if result.is_none() {
        body.shutdown().await;
    }
    (body, result)
}

pub(crate) struct WarmBody {
    commands: tokio::sync::mpsc::Sender<Command>,
    grant: PlaybackSource,
    stop: watch::Sender<bool>,
    completion: Completion,
    first_emitted: bool,
}

impl WarmBody {
    async fn check_prefix(&mut self) -> Result<bool> {
        self.grant.warm_check().await.map(|()| true)
    }

    pub(crate) async fn prefix(&mut self, prefix: Bytes) -> Result<Option<Bytes>> {
        self.check_prefix().await?;
        self.first_emitted = true;
        Ok(Some(prefix))
    }

    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>> {
        let (reply, received) = tokio::sync::oneshot::channel();
        self.commands.send(Command::Read(reply)).await.map_err(|_| Error::NotFound)?;
        match received.await.map_err(|_| Error::NotFound)? {
            Ok(Some(bytes)) => {
                if !self.first_emitted {
                    self.check_prefix().await?;
                }
                self.first_emitted = true;
                Ok(Some(bytes))
            }
            result => result,
        }
    }

    pub(crate) async fn shutdown(&mut self) {
        self.stop.send_replace(true);
        let _ = self.completion.clone().await;
    }
}

impl Drop for WarmBody {
    fn drop(&mut self) {
        self.stop.send_replace(true);
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
    completion: Mutex<Option<Completion>>,
    monitors: Arc<crate::playback_access::relay::MonitorRegistry>,
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
    stop: watch::Sender<bool>,
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
        let (claimed, mut claim_rx) = watch::channel(false);
        let (commands, mut command_rx) = tokio::sync::mpsc::channel(2);
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
            completion: Mutex::new(None),
            monitors: Arc::new(crate::playback_access::relay::MonitorRegistry::default()),
            commands: commands.clone(),
        });
        let worker_entry = entry.clone();
        let worker = async move {
            let mut lease_cancel = lease_cancel;
            let mut cancel_rx = cancel_rx;
            if *lease_cancel.borrow() || *cancel_rx.borrow() {
                worker_entry.stop();
            }
            let opened = if *lease_cancel.borrow() || *cancel_rx.borrow() {
                None
            } else {
                tokio::select! {
                    biased;
                    _ = lease_cancel.changed() => None,
                    _ = cancel_rx.changed() => None,
                    _ = tokio::time::sleep_until(start_deadline) => None,
                    result = tokio::time::timeout_at(start_deadline, async {
                        grant.warm_check().await?;
                        grant
                            .warm_open(AudioRequest::default(), worker_entry.monitors.clone())
                            .await
                    }) => result.ok().and_then(Result::ok),
                }
            };
            if let Some((OpenedAudio::Content(mut content), ticket)) = opened {
                if let Ok(mut current) = worker_entry.ticket.lock() {
                    *current = Some(ticket.clone());
                }
                let mut ticket_cancel = ticket.cancelled();
                if let Some(body) = content.body.take() {
                    *worker_entry.ready.lock().await = Some(Ready {
                        content,
                        prefix: None,
                    });
                    worker_entry.changed.notify_one();
                    let mut first_read: Option<FirstRead> = Some(
                        read_first(
                            body,
                            lease_cancel.clone(),
                            cancel_rx.clone(),
                            ticket_cancel.clone(),
                        )
                        .boxed(),
                    );
                    let mut body = None;
                    let mut first_result = None;
                    let mut waiting_reader = None;
                    let mut claimed = ticket.is_claimed() || *claim_rx.borrow();
                    let mut first_ended = false;
                    let mut terminal = false;
                    while first_read.is_some() {
                        if *lease_cancel.borrow() || *cancel_rx.borrow() || *ticket_cancel.borrow() {
                            terminal = true;
                            break;
                        }
                        tokio::select! {
                            biased;
                            _ = lease_cancel.changed() => { terminal = true; break; },
                            _ = cancel_rx.changed() => { terminal = true; break; },
                            _ = ticket_cancel.changed() => { terminal = true; break; },
                            _ = tokio::time::sleep_until(deadline), if !claimed => { worker_entry.stop(); terminal = true; break; },
                            _ = tokio::time::sleep_until(start_deadline), if !claimed => { worker_entry.stop(); terminal = true; break; },
                            _ = claim_rx.changed(), if !claimed => claimed = true,
                            command = command_rx.recv() => match command {
                                Some(Command::Read(reply)) if waiting_reader.is_none() => waiting_reader = Some(reply),
                                Some(Command::Read(reply)) => {
                                    let _ = reply.send(Err(Error::Conflict("Audio body is busy. Try again.".into())));
                                }
                                None => { terminal = true; break; },
                            },
                            (stream, result) = first_read.as_mut().expect("pending warm read") => {
                                first_read.take();
                                body = Some(stream);
                                ticket.finish_preparation();
                                let Some(result) = result else {
                                    terminal = true;
                                    break;
                                };
                                claimed |= ticket.is_claimed() || *claim_rx.borrow();
                                let result = if matches!(&result, Ok(Some(_))) {
                                    tokio::select! {
                                        biased;
                                        _ = lease_cancel.changed() => { terminal = true; Err(Error::NotFound) },
                                        _ = cancel_rx.changed() => { terminal = true; Err(Error::NotFound) },
                                        _ = ticket_cancel.changed() => { terminal = true; Err(Error::NotFound) },
                                        checked = grant.warm_check() => match checked {
                                        Ok(()) => result,
                                        Err(_) if !claimed => { terminal = true; Err(Error::NotFound) },
                                        Err(_) => Err(Error::NotFound),
                                        },
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
                    if terminal {
                        if let Some(first_read) = first_read.take() {
                            let (mut stream, _) = first_read.await;
                            stream.shutdown().await;
                        } else if let Some(body) = &mut body {
                            body.shutdown().await;
                        }
                        if let Some(reply) = waiting_reader.take() {
                            let _ = reply.send(Err(Error::NotFound));
                        }
                    }
                    if !terminal && !claimed && !ticket.is_claimed() && !*claim_rx.borrow() {
                        let retained_prefix = worker_entry
                            .ready
                            .lock()
                            .await
                            .as_ref()
                            .is_some_and(|ready| ready.prefix.is_some());
                        if *lease_cancel.borrow() || *cancel_rx.borrow() || *ticket_cancel.borrow() {
                            terminal = true;
                        }
                        if !terminal && (first_result.is_some() || retained_prefix) {
                            tokio::select! {
                                biased;
                                _ = lease_cancel.changed() => terminal = true,
                                _ = cancel_rx.changed() => terminal = true,
                                _ = ticket_cancel.changed() => terminal = true,
                                _ = tokio::time::sleep_until(deadline) => { worker_entry.stop(); terminal = true; },
                                _ = claim_rx.changed() => claimed = true,
                            }
                        }
                    }
                    if *lease_cancel.borrow() || *cancel_rx.borrow() || *ticket_cancel.borrow() {
                        terminal = true;
                    }
                    claimed |= ticket.is_claimed() || *claim_rx.borrow();
                    if !terminal && claimed && !first_ended {
                        loop {
                            if *lease_cancel.borrow() || *cancel_rx.borrow() || *ticket_cancel.borrow() {
                                break;
                            }
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
                                                Some(Command::Read(extra)) => {
                                                    let _ = extra.send(Err(Error::Conflict("Audio body is busy. Try again.".into())));
                                                    Err(Error::NotFound)
                                                }
                                                None => Err(Error::NotFound),
                                            },
                                            result = body.as_mut().expect("claimed warm body").chunk() => result,
                                        }
                                    };
                                    let ended = !matches!(&result, Ok(Some(_)));
                                    let _ = reply.send(result);
                                    if ended {
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                    drop(first_read);
                    if let Some(body) = &mut body {
                        body.shutdown().await;
                    }
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
            let _ = worker_entry.monitors.close().await;
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
        };
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
        let task = tokio::spawn(worker);
        let finished = task.abort_handle();
        let completion: Completion = async move {
            task.await.map_err(|_| Arc::new(Error::NotFound))?;
            Ok(())
        }
        .boxed()
        .shared();
        if let Ok(mut current) = entry.completion.lock() {
            *current = Some(completion.clone());
        }
        state.completions.retain(|_, work| !work.finished.is_finished());
        state.completions.insert(
            lease,
            Work {
                finished,
                completion,
                stop: entry.canceled.clone(),
            },
        );
        drop(state);
        tracing::debug!(
            event = "provider_audio_warm_started",
            outcome = "reserved",
            reserved_bytes = RESERVATION,
            "Provider audio warm work reserved"
        );
    }

    pub(crate) async fn claim(
        &self,
        actor: &Actor,
        lease: Uuid,
        grant: &PlaybackSource,
        request: AudioRequest,
    ) -> Result<Option<Taken>> {
        if request.head || request.range.is_some() {
            self.retire_unclaimed(lease).await?;
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
            self.retire_unclaimed(lease).await?;
            return Ok(None);
        }
        let ticket = entry
            .ticket
            .lock()
            .ok()
            .and_then(|ticket| ticket.as_ref().cloned());
        let Some(ticket) = ticket else {
            self.retire_unclaimed(lease).await?;
            return Ok(None);
        };
        let mut ticket_cancel = ticket.cancelled();
        let mut entry_cancel = entry.canceled.subscribe();
        let wait = async {
            loop {
                let changed = entry.changed.notified();
                tokio::pin!(changed);
                if *ticket_cancel.borrow() || *entry_cancel.borrow() {
                    return Ok::<_, Error>(None);
                }
                let authorized = tokio::select! {
                    biased;
                    _ = ticket_cancel.changed() => return Ok(None),
                    _ = entry_cancel.changed() => return Ok(None),
                    result = grant.check() => result,
                };
                authorized?;
                let eligible = tokio::select! {
                    biased;
                    _ = ticket_cancel.changed() => return Ok(None),
                    _ = entry_cancel.changed() => return Ok(None),
                    result = grant.warm_check() => result.is_ok(),
                };
                if !eligible {
                    return Ok(None);
                }
                if let Some(ready) = entry.ready.lock().await.take() {
                    let eligible = tokio::select! {
                        biased;
                        _ = ticket_cancel.changed() => return Ok(None),
                        _ = entry_cancel.changed() => return Ok(None),
                        result = grant.warm_check() => result.is_ok(),
                    };
                    if !eligible {
                        drop(ready);
                        return Ok(None);
                    }
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
                self.retire_unclaimed(lease).await?;
                return Err(error);
            }
            Err(_) => None,
        };
        let Some(ready) = ready else {
            self.retire_unclaimed(lease).await?;
            return Ok(None);
        };
        let claimed = {
            let mut state = self.state.lock().map_err(|_| Error::NotFound)?;
            let current = state
                .entries
                .get(&lease)
                .is_some_and(|current| Arc::ptr_eq(current, &entry));
            if current && ticket.claim() {
                state.entries.remove(&lease);
                true
            } else {
                false
            }
        };
        if !claimed {
            drop(ready);
            self.retire_unclaimed(lease).await?;
            return Ok(None);
        }
        entry.claimed.send_replace(true);
        Ok(Some(Taken {
            content: ready.content,
            prefix: ready.prefix,
            body: WarmBody {
                commands: entry.commands.clone(),
                grant: entry.grant.clone(),
                stop: entry.canceled.clone(),
                completion: entry
                    .completion
                    .lock()
                    .map_err(|_| Error::NotFound)?
                    .clone()
                    .ok_or(Error::NotFound)?,
                first_emitted: false,
            },
            reservation: entry.reservation.clone(),
        }))
    }

    pub(crate) async fn retire_and_join(&self, lease: Uuid) -> Result<()> {
        let (entry, completion) = {
            let mut state = self.state.lock().map_err(|_| Error::NotFound)?;
            let entry = state.entries.remove(&lease);
            let completion = state.completions.get(&lease).map(|work| {
                work.stop.send_replace(true);
                work.completion.clone()
            });
            (entry, completion)
        };
        drop(entry);
        if let Some(completion) = completion {
            completion.await.map_err(|_| Error::NotFound)?;
            if let Ok(mut state) = self.state.lock() {
                state.completions.remove(&lease);
            }
        }
        Ok(())
    }

    async fn retire_unclaimed(&self, lease: Uuid) -> Result<()> {
        let (entry, completion) = {
            let mut state = self.state.lock().map_err(|_| Error::NotFound)?;
            let entry = state.entries.remove(&lease);
            let completion = if let Some(entry) = &entry {
                entry.stop();
                state
                    .completions
                    .get(&lease)
                    .map(|work| work.completion.clone())
            } else {
                None
            };
            (entry, completion)
        };
        drop(entry);
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
            let completions = state.completions.values().map(|work| {
                work.stop.send_replace(true);
                work.completion.clone()
            }).collect::<Vec<_>>();
            for entry in &entries {
                entry.stop();
            }
            (entries, completions)
        };
        drop(entries);
        for completion in completions {
            completion.await.map_err(|_| Error::NotFound)?;
        }
        if let Ok(mut state) = self.state.lock() {
            state.completions.retain(|_, work| !work.finished.is_finished());
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
