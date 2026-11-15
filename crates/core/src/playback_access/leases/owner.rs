use super::{Entry, file::RetainedFile, stopped};
use crate::playback_access::conversion::{
    self, ConversionLimits, ConversionStream, Negotiated, PositionedAudio,
};
use crate::{
    AudioContent, AudioRequest, AudioStream, AudioValidators, Error, MediaTools, OpenedAudio,
    QualityRequest, Result,
};
use bytes::Bytes;
use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    sync::{Notify, mpsc, oneshot, watch},
    task::JoinSet,
};
use uuid::Uuid;

pub(super) enum Command {
    Open(AudioRequest, oneshot::Sender<Result<OpenedAudio>>),
    OpenQuality(QualityOpen, oneshot::Sender<Result<LeaseConversion>>),
    Read(Uuid, oneshot::Sender<Result<Option<Bytes>>>),
}

pub(super) struct QualityOpen {
    pub request: QualityRequest,
    pub limits: ConversionLimits,
    pub tools: MediaTools,
}

pub(super) enum LeaseConversion {
    Converted(Option<AudioStream>),
    Unchanged(&'static str),
}

pub(super) struct BodyControl {
    closed: AtomicBool,
    wake: Arc<Notify>,
}
impl BodyControl {
    pub fn check(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            Err(Error::NotFound)
        } else {
            Ok(())
        }
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake.notify_one();
    }
}

pub(crate) struct LeaseBody {
    entry: Weak<Entry>,
    id: Uuid,
    commands: mpsc::Sender<Command>,
    control: Arc<BodyControl>,
}
impl LeaseBody {
    fn check(&self) -> Result<()> {
        self.control.check()?;
        self.entry.upgrade().ok_or(Error::NotFound)?.alive()
    }

    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>> {
        self.check()?;
        let mut cancel = CancelRead(Some(self.control.clone()));
        let (reply, read) = oneshot::channel();
        self.commands
            .send(Command::Read(self.id, reply))
            .await
            .map_err(|_| Error::NotFound)?;
        let result = read.await.map_err(|_| Error::NotFound)?;
        self.check()?;
        cancel.0 = None;
        result
    }
}
impl Drop for LeaseBody {
    fn drop(&mut self) {
        self.control.close();
    }
}
struct CancelRead(Option<Arc<BodyControl>>);
impl Drop for CancelRead {
    fn drop(&mut self) {
        if let Some(control) = &self.0 {
            control.close();
        }
    }
}

enum Input {
    File { position: u64, remaining: u64 },
    Remote(AudioStream),
    Converted(Box<ConversionStream>),
}
struct Stream {
    input: Input,
    control: Arc<BodyControl>,
}

pub(super) async fn serve(
    entry: &Arc<Entry>,
    mut commands: mpsc::Receiver<Command>,
    file: Option<Arc<RetainedFile>>,
) {
    let mut streams = HashMap::<Uuid, Stream>::new();
    let mut pending = JoinSet::<Pending>::new();
    let mut pending_bodies = 0usize;
    let mut stop = entry.cancel.subscribe();
    let grant = entry.grant.get().expect("published lease has its grant");
    loop {
        let closed: Vec<_> = streams
            .iter()
            .filter_map(|(id, stream)| stream.control.check().is_err().then_some(*id))
            .collect();
        for id in closed {
            if let Some(stream) = streams.remove(&id) {
                retire_stream(stream).await;
            }
        }
        if entry.alive().is_err() {
            break;
        }
        let command = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = tokio::time::sleep_until(entry.deadline) => break,
            _ = grant.revoked() => { entry.stop(); break; },
            Some(done) = pending.join_next(), if !pending.is_empty() => {
                let Ok(Pending { reply, head, result }) = done else { continue };
                if !head {
                    pending_bodies -= 1;
                }
                let fenced = match &file {
                    Some(file) => file.check(),
                    None => Ok(()),
                };
                let result = match fenced {
                    Ok(()) => result,
                    Err(error) => {
                        entry.stop();
                        tracing::debug!(event = "lease_conversion_fenced", error_kind = error.code(), "Retained file changed before publication");
                        if let Ok(Negotiated::Converted(Some(stream))) = result {
                            (*stream).close().await;
                        }
                        Err(error)
                    }
                };
                let result = match result {
                    Ok(Negotiated::Converted(Some(stream))) => match entry.check().await {
                        Ok(()) if !reply.is_closed() => {
                            let id = Uuid::new_v4();
                            let control = Arc::new(BodyControl {
                                closed: AtomicBool::new(false),
                                wake: entry.wake.clone(),
                            });
                            streams.insert(
                                id,
                                Stream {
                                    input: Input::Converted(stream),
                                    control: control.clone(),
                                },
                            );
                            Ok(LeaseConversion::Converted(Some(AudioStream::lease(
                                LeaseBody {
                                    entry: Arc::downgrade(entry),
                                    id,
                                    commands: entry.commands.clone(),
                                    control,
                                },
                                None,
                            ))))
                        }
                        Ok(()) => {
                            (*stream).close().await;
                            continue;
                        }
                        Err(error) => {
                            (*stream).close().await;
                            Err(error)
                        }
                    },
                    Ok(Negotiated::Converted(None)) => Ok(LeaseConversion::Converted(None)),
                    Ok(Negotiated::Unchanged(reason)) => Ok(LeaseConversion::Unchanged(reason)),
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
                #[cfg(test)]
                entry.publications.opened.notify_one();
                continue;
            }
            _ = entry.wake.notified() => continue,
            command = commands.recv() => match command { Some(command) => command, None => break },
        };
        match command {
            Command::Open(request, mut reply) => {
                if reply.is_closed() {
                    continue;
                }
                let result = async {
                    entry.check().await?;
                    if streams.len() + pending_bodies >= 2 && !request.head {
                        return Err(Error::Conflict(
                            "This playback lease already has two audio bodies".into(),
                        ));
                    }
                    if let Some(file) = &file {
                        if let Err(error) = file.check() {
                            entry.stop();
                            return Err(error);
                        }
                        return file_content(file, request);
                    }
                    if let Some(mut taken) = entry
                        .warm
                        .claim(&entry.actor, entry.metadata.id, grant, request)
                        .await?
                    {
                        let expected = taken.content.length;
                        taken.content.body = Some(AudioStream::prepared(
                            taken.prefix,
                            taken.body,
                            taken.reservation,
                            expected,
                        ));
                        Ok(OpenedAudio::Content(taken.content))
                    } else {
                        grant.open(request).await
                    }
                };
                let result = tokio::select! {
                    biased;
                    _ = stop.changed() => Err(Error::NotFound),
                    _ = tokio::time::sleep_until(entry.deadline) => Err(Error::NotFound),
                    _ = reply.closed() => continue,
                    result = result => result,
                };
                let result = match result {
                    Ok(mut opened) => match entry.check().await {
                        Ok(()) if !reply.is_closed() => {
                            if let OpenedAudio::Content(audio) = &mut opened
                                && !request.head
                            {
                                let input = match &file {
                                    Some(_) => Some(Input::File {
                                        position: audio
                                            .range
                                            .as_ref()
                                            .map_or(0, |range| *range.start()),
                                        remaining: audio.length.unwrap_or(0),
                                    }),
                                    None => audio.body.take().map(Input::Remote),
                                };
                                if let Some(input) = input {
                                    let id = Uuid::new_v4();
                                    let control = Arc::new(BodyControl {
                                        closed: AtomicBool::new(false),
                                        wake: entry.wake.clone(),
                                    });
                                    streams.insert(
                                        id,
                                        Stream {
                                            input,
                                            control: control.clone(),
                                        },
                                    );
                                    audio.body = Some(AudioStream::lease(
                                        LeaseBody {
                                            entry: Arc::downgrade(entry),
                                            id,
                                            commands: entry.commands.clone(),
                                            control,
                                        },
                                        audio.length,
                                    ));
                                }
                            }
                            Ok(opened)
                        }
                        Ok(()) => continue,
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
                #[cfg(test)]
                entry.publications.opened.notify_one();
            }
            Command::OpenQuality(open, reply) => {
                if reply.is_closed() {
                    continue;
                }
                let head = open.request.original.head;
                let admitted = async {
                    entry.check().await?;
                    if streams.len() + pending_bodies >= 2 && !head {
                        return Err(Error::Conflict(
                            "This playback lease already has two audio bodies".into(),
                        ));
                    }
                    Ok(())
                };
                if let Err(error) = admitted.await {
                    let _ = reply.send(Err(error));
                    continue;
                }
                if !head {
                    pending_bodies += 1;
                }
                pending.spawn(negotiation(
                    entry.clone(),
                    grant.clone(),
                    file.clone(),
                    open,
                    reply,
                ));
            }
            Command::Read(id, mut reply) => {
                if reply.is_closed() {
                    continue;
                }
                let Some(mut stream) = streams.remove(&id) else {
                    let _ = reply.send(Err(Error::NotFound));
                    continue;
                };
                let result = async {
                    entry.check().await?;
                    stream.control.check()?;
                    let bytes = match &mut stream.input {
                        Input::File { position, remaining } => {
                            if *remaining == 0 { return Ok(None); }
                            let file = file.as_ref().ok_or_else(stopped)?;
                            let bytes = file.read(entry.clone(), stream.control.clone(), *position, (*remaining).min(65536) as usize).await?;
                            *position += bytes.len() as u64;
                            *remaining -= bytes.len() as u64;
                            (!bytes.is_empty()).then_some(bytes)
                        }
                        Input::Remote(body) => {
                            tokio::select! {
                                biased;
                                _ = stop.changed() => return Err(Error::NotFound),
                                _ = tokio::time::sleep_until(entry.deadline) => return Err(Error::NotFound),
                                _ = reply.closed() => return Err(Error::NotFound),
                                result = body.chunk() => result?,
                            }
                        }
                        Input::Converted(body) => {
                            let file = file.as_ref().ok_or_else(stopped)?;
                            if let Err(error) = file.check() {
                                entry.stop();
                                return Err(error);
                            }
                            let bytes = tokio::select! {
                                biased;
                                _ = stop.changed() => return Err(Error::NotFound),
                                _ = tokio::time::sleep_until(entry.deadline) => return Err(Error::NotFound),
                                _ = reply.closed() => return Err(Error::NotFound),
                                result = body.chunk() => result?,
                            };
                            if let Err(error) = file.check() {
                                entry.stop();
                                return Err(error);
                            }
                            bytes
                        }
                    };
                    entry.check().await?;
                    stream.control.check()?;
                    Ok(bytes)
                }.await;
                if matches!(&result, Ok(Some(_))) && !reply.is_closed() {
                    streams.insert(id, stream);
                } else {
                    retire_stream(stream).await;
                }
                let _ = reply.send(result);
                #[cfg(test)]
                entry.publications.read.notify_one();
            }
        }
    }
    entry.stop();
    while let Some(done) = pending.join_next().await {
        if let Ok(Pending { reply, result, .. }) = done {
            if let Ok(Negotiated::Converted(Some(stream))) = result {
                (*stream).close().await;
            }
            let _ = reply.send(Err(Error::NotFound));
        }
    }
    for (_, stream) in streams.drain() {
        retire_stream(stream).await;
    }
    let _ = entry.warm.retire_lease(entry.metadata.id).await;
    tracing::debug!(
        event = "playback_lease_retired",
        playback_lease = entry.correlation,
        playback_grant = grant.correlation_id(),
        bodies = streams.len(),
        "Playback lease retired"
    );
}

struct Pending {
    reply: oneshot::Sender<Result<LeaseConversion>>,
    head: bool,
    result: Result<Negotiated>,
}

async fn negotiation(
    entry: Arc<Entry>,
    grant: crate::PlaybackSource,
    file: Option<Arc<RetainedFile>>,
    open: QualityOpen,
    mut reply: oneshot::Sender<Result<LeaseConversion>>,
) -> Pending {
    let head = open.request.original.head;
    let (cancel, mut cancelled) = watch::channel(false);
    let mut stop = entry.cancel.subscribe();
    let work = async {
        let Some(file) = file else {
            return Ok(Negotiated::Unchanged("remote-source"));
        };
        if let Err(error) = file.check() {
            entry.stop();
            return Err(error);
        }
        let positioned: Arc<dyn PositionedAudio> = file;
        conversion::negotiate(
            &open.limits,
            &open.tools,
            &entry.actor,
            &grant,
            conversion::Input::Positioned(&positioned),
            open.request,
            &mut cancelled,
        )
        .await
    };
    tokio::pin!(work);
    let deadline = tokio::time::sleep_until(entry.deadline);
    tokio::pin!(deadline);
    let result = loop {
        let requested = *cancel.borrow();
        tokio::select! {
            biased;
            result = &mut work => break result,
            _ = conversion::cancelled(&mut stop), if !requested => { let _ = cancel.send(true); }
            _ = &mut deadline, if !requested => { let _ = cancel.send(true); }
            _ = reply.closed(), if !requested => { let _ = cancel.send(true); }
        }
    };
    Pending {
        reply,
        head,
        result,
    }
}

async fn retire_stream(stream: Stream) {
    match stream.input {
        Input::Remote(mut body) => body.shutdown().await,
        Input::Converted(body) => (*body).close().await,
        Input::File { .. } => {}
    }
}

fn file_content(file: &RetainedFile, request: AudioRequest) -> Result<OpenedAudio> {
    let range = if request.head {
        None
    } else if let Some(selection) = request.range {
        let Some(range) = selection.resolve(file.length) else {
            return Ok(OpenedAudio::Unsatisfiable {
                length: Some(file.length),
            });
        };
        Some(range)
    } else {
        None
    };
    let length = range
        .as_ref()
        .map_or(file.length, |range| range.end() - range.start() + 1);
    Ok(OpenedAudio::Content(Box::new(AudioContent {
        content_type: file.content_type,
        length: Some(length),
        total_length: Some(file.length),
        range,
        seekable: true,
        validators: AudioValidators::default(),
        body: None,
    })))
}
