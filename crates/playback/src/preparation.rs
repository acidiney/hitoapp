use crate::{Error, Result, Source, SourceStop, grants::Grants};
use hito_audio::{Decoder, Player};
use hito_core::PlaybackSource;
use std::thread::{self, JoinHandle};
use tokio::sync::oneshot;

type Reply = oneshot::Sender<Result<bool>>;

#[derive(Default)]
pub(crate) struct Preparation {
    pending: Option<Pending>,
}

struct Pending {
    stop: Option<SourceStop>,
    grant: PlaybackSource,
    worker: Option<JoinHandle<hito_audio::Result<Decoder>>>,
    reply: Option<Reply>,
}

impl Preparation {
    pub fn start(&mut self, mut input: Source, reply: Reply) {
        self.cancel();
        let stop = input.stop.child();
        input.stop = stop.clone();
        let grant = input.grant.clone();
        let cancelled = stop.clone();
        let span = tracing::debug_span!("native_preparation", source_id = grant.source_id());
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        let worker = thread::Builder::new()
            .name("hito-prepare".into())
            .spawn(move || {
                tracing::dispatcher::with_default(&dispatch, || {
                    let _entered = span.enter();
                    crate::pcm::decode(input, &mut || cancelled.requested())
                })
            });
        match worker {
            Ok(worker) => {
                self.pending = Some(Pending {
                    stop: Some(stop),
                    grant,
                    worker: Some(worker),
                    reply: Some(reply),
                });
                tracing::debug!(
                    event = "native_preparation_started",
                    "Preparing the next native input"
                );
            }
            Err(_) => {
                let _ = reply.send(Err(Error::Worker));
            }
        }
    }

    pub fn poll(&mut self, player: &mut Player, grants: &Grants) {
        let Some(pending) = &self.pending else {
            return;
        };
        if pending.reply.as_ref().is_none_or(Reply::is_closed) {
            self.cancel();
            return;
        }
        if pending
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        let mut pending = self.pending.take().unwrap();
        let decoded = pending
            .worker
            .take()
            .ok_or(Error::Worker)
            .and_then(|worker| {
                worker
                    .join()
                    .map_err(|_| Error::Worker)?
                    .map_err(Error::Native)
            });
        let result = if pending.reply.as_ref().is_none_or(Reply::is_closed) {
            drop(decoded);
            Err(Error::Stopped)
        } else {
            decoded.and_then(|decoder| player.prepare_next_decoder(decoder).map_err(Error::Native))
        };
        if let Ok(replaced) = result {
            grants.push(pending.grant.clone(), replaced);
            pending.stop = None;
        }
        tracing::debug!(
            event = "native_preparation_finished",
            accepted = result.is_ok(),
            "Next native input prepared"
        );
        if let Some(reply) = pending.reply.take() {
            let _ = reply.send(result);
        }
    }

    pub fn cancel(&mut self) -> bool {
        let pending = self.pending.take();
        let cancelled = pending.is_some();
        drop(pending);
        if cancelled {
            tracing::debug!(
                event = "native_preparation_cancelled",
                "Next native input reclaimed"
            );
        }
        cancelled
    }
}

impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.request();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Err(Error::Stopped));
        }
    }
}
