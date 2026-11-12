use super::{Entry, file::RetainedFile, stopped};
use crate::{AudioContent, AudioRequest, AudioStream, AudioValidators, Error, OpenedAudio, Result};
use bytes::Bytes;
use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Notify, mpsc, oneshot};
use uuid::Uuid;

pub(super) enum Command {
    Open(AudioRequest, oneshot::Sender<Result<OpenedAudio>>),
    Read(Uuid, oneshot::Sender<Result<Option<Bytes>>>),
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
    let mut stop = entry.cancel.subscribe();
    let grant = entry.grant.get().expect("published lease has its grant");
    loop {
        streams.retain(|_, stream| stream.control.check().is_ok());
        if entry.alive().is_err() {
            break;
        }
        let command = tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = tokio::time::sleep_until(entry.deadline) => break,
            _ = grant.revoked() => { entry.stop(); break; },
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
                    if streams.len() >= 2 && !request.head {
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
                    grant.open(request).await
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
                    };
                    entry.check().await?;
                    stream.control.check()?;
                    Ok(bytes)
                }.await;
                if matches!(&result, Ok(Some(_))) && !reply.is_closed() {
                    streams.insert(id, stream);
                }
                let _ = reply.send(result);
                #[cfg(test)]
                entry.publications.read.notify_one();
            }
        }
    }
    entry.stop();
    tracing::debug!(
        event = "playback_lease_retired",
        playback_lease = entry.correlation,
        playback_grant = grant.correlation_id(),
        bodies = streams.len(),
        "Playback lease retired"
    );
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
