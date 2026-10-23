use super::{
    Action, PlayerAudiobookContinuation, PlayerAudiobookRequest, PlayerError, PlayerQueueInput,
    Request, Response, Result, attachment::Owned, ownership::State, progress::Progress,
};
use hito_core::{Actor, Services};
use std::{future::Future, pin::Pin};
use tokio::time::{Duration, Instant};
use tracing::Instrument;
use uuid::Uuid;

const PREPARATION_LIFETIME: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
struct Fence {
    scope: Uuid,
    lease: Uuid,
    revision: u64,
    selection: u64,
    sequence: u64,
}
impl Fence {
    fn capture(owned: &Owned) -> Self {
        let state = owned.queue.snapshot();
        Self {
            scope: owned.scope,
            lease: owned.lease,
            revision: state.revision,
            selection: state.selection,
            sequence: owned.sequence,
        }
    }
    fn matches(&self, owned: &Owned) -> bool {
        self.scope == owned.scope
            && self.lease == owned.lease
            && self.revision == owned.queue.revision()
            && self.selection == owned.queue.snapshot().selection
            && self.sequence == owned.sequence
            && owned.authority_available
    }
}

pub(super) struct Prepared {
    pub(super) request_id: Uuid,
    fence: Fence,
    expires: Instant,
    pub info: PlayerAudiobookContinuation,
    pub entries: Vec<PlayerQueueInput>,
    pub start: usize,
}
impl Prepared {
    pub fn check(&self, owned: &Owned, token: Uuid) -> Result<()> {
        if self.info.token != token || self.expires <= Instant::now() || !self.fence.matches(owned)
        {
            return Err(PlayerError::Conflict);
        }
        Ok(())
    }
}

type Read = Pin<Box<dyn Future<Output = Result<Prepared>> + Send>>;
struct Job {
    request: Request,
    input: PlayerAudiobookRequest,
    fence: Fence,
    watermark: u64,
    expires: Instant,
    read: Option<Read>,
}
pub(super) struct Continuation {
    services: Services,
    job: Option<Job>,
}
impl Continuation {
    pub fn new(services: Services) -> Self {
        Self {
            services,
            job: None,
        }
    }
    pub fn prepare(&mut self, state: &mut State, progress: &mut Progress, request: Request) {
        let Action::PrepareAudiobook(input) = &request.action else {
            unreachable!()
        };
        let result = state.owned(&request.actor, input.lease).and_then(|owned| {
            if !progress.ready() {
                return Err(PlayerError::Busy);
            }
            let fence = Fence::capture(owned);
            if !owned.authority_available {
                return Err(PlayerError::Unavailable);
            }
            if fence.revision != input.revision || fence.selection != input.selection {
                return Err(PlayerError::Conflict);
            }
            Ok(fence)
        });
        let fence = match result {
            Ok(fence) => fence,
            Err(error) => {
                let _ = request.reply.send(Err(error));
                return;
            }
        };
        self.retire(PlayerError::Conflict);
        state.owner.as_mut().unwrap().continuation = None;
        tracing::debug!(event = "player_audiobook_prepare_started", request_id = %input.id,
            account_id = request.actor.user().id, "Preparing audiobook continuation");
        self.job = Some(Job {
            input: input.clone(),
            fence,
            watermark: progress.barrier(),
            expires: Instant::now() + PREPARATION_LIFETIME,
            read: None,
            request,
        });
    }
    pub fn cancel(
        &mut self,
        state: &mut State,
        actor: &Actor,
        lease: Uuid,
        id: Uuid,
    ) -> Result<()> {
        state.owned(actor, lease)?;
        if self.job.as_ref().is_some_and(|job| job.input.id == id) {
            self.retire(PlayerError::Conflict);
        }
        if state
            .owner
            .as_ref()
            .and_then(|owned| owned.continuation.as_ref())
            .is_some_and(|prepared| prepared.request_id == id)
        {
            state.owner.as_mut().unwrap().continuation = None;
        }
        Ok(())
    }
    pub fn settle(
        &mut self,
        state: &mut State,
        actor: &Actor,
        lease: Uuid,
        id: Uuid,
        command: super::PlayerCommand,
    ) -> Result<Option<super::PlayerReceipt>> {
        let owned = state.owned(actor, lease)?;
        if let Some(accepted) = owned
            .receipts
            .iter()
            .find(|accepted| accepted.command.id == command.id)
        {
            return if accepted.command == command && accepted.audiobook_request == Some(id) {
                Ok(Some(accepted.receipt.clone()))
            } else {
                Err(PlayerError::Conflict)
            };
        }
        if let Some((request, retired)) = owned
            .retired_audiobooks
            .iter()
            .find(|(request, retired)| *request == id || retired.id == command.id)
        {
            return if *request == id && retired == &command {
                Ok(None)
            } else {
                Err(PlayerError::Conflict)
            };
        }
        owned.check(&command)?;
        let prepared = owned.continuation.as_ref().ok_or(PlayerError::Conflict)?;
        let super::PlayerAction::ContinueAudiobook { token } = command.action else {
            return Err(PlayerError::Invalid);
        };
        if prepared.request_id != id || prepared.info.token != token {
            return Err(PlayerError::Conflict);
        }
        self.cancel(state, actor, lease, id)?;
        let owned = state.owner.as_mut().unwrap();
        if owned.retired_audiobooks.len() == 64 {
            owned.retired_audiobooks.pop_front();
        }
        owned.retired_audiobooks.push_back((id, command));
        Ok(None)
    }
    pub fn sync(&mut self, state: &mut State, progress: &Progress) {
        if state.owner.as_ref().is_some_and(|owned| {
            owned
                .continuation
                .as_ref()
                .is_some_and(|prepared| prepared.check(owned, prepared.info.token).is_err())
        }) {
            state.owner.as_mut().unwrap().continuation = None;
        }
        let Some(job) = &self.job else {
            return;
        };
        if job.request.reply.is_closed()
            || state.owner.as_ref().is_none_or(|owned| {
                !owned.actor.same_session(&job.request.actor) || !job.fence.matches(owned)
            })
        {
            self.retire(PlayerError::Conflict);
            return;
        }
        let job = self.job.as_mut().unwrap();
        if job.read.is_none() && progress.settled(job.watermark) {
            let services = self.services.clone();
            let actor = job.request.actor.clone();
            let input = job.input.clone();
            let fence = job.fence;
            job.read = Some(Box::pin(
                read(services, actor, input, fence).instrument(job.request.span.clone()),
            ));
        }
    }
    pub async fn next(&mut self) -> (Request, Result<Prepared>) {
        let Some(job) = &mut self.job else {
            return std::future::pending().await;
        };
        let result = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(job.expires) => Err(PlayerError::Unavailable),
            result = async { job.read.as_mut().unwrap().await }, if job.read.is_some() => result,
        };
        let job = self.job.take().unwrap();
        (job.request, result)
    }
    pub fn complete(state: &mut State, request: Request, result: Result<Prepared>) {
        let span = request.span.clone();
        let _entered = span.enter();
        let result = result.and_then(|prepared| {
            let owned = state
                .owner
                .as_mut()
                .filter(|owned| owned.actor.same_session(&request.actor))
                .ok_or(PlayerError::Conflict)?;
            prepared.check(owned, prepared.info.token)?;
            let response = prepared.info.clone();
            if request.reply.is_closed() {
                return Err(PlayerError::Conflict);
            }
            owned.continuation = Some(prepared);
            Ok(Response::AudiobookContinuation(response))
        });
        tracing::debug!(event = "player_audiobook_prepare_finished", success = result.is_ok(), error_kind = ?result.as_ref().err(),
            elapsed_ms = request.started.elapsed().as_millis() as u64, "Audiobook continuation prepared");
        if request.reply.send(result).is_err()
            && let Some(owned) = &mut state.owner
        {
            owned.continuation = None;
        }
    }
    fn retire(&mut self, error: PlayerError) {
        if let Some(job) = self.job.take() {
            tracing::debug!(event = "player_audiobook_prepare_retired", request_id = %job.input.id,
                elapsed_ms = job.request.started.elapsed().as_millis() as u64, "Audiobook preparation retired");
            let _ = job.request.reply.send(Err(error));
        }
    }
}

async fn read(
    services: Services,
    actor: Actor,
    input: PlayerAudiobookRequest,
    fence: Fence,
) -> Result<Prepared> {
    let playback = services
        .collection
        .audiobook_playback(&actor, &input.audiobook_id.to_string())
        .await
        .map_err(PlayerError::from)?;
    let progress = playback.progress;
    let rate = (progress.speed * 1000.).round() as u16;
    if !(500..=3000).contains(&rate) || f64::from(rate) / 1000. != progress.speed {
        return Err(PlayerError::Unsupported);
    }
    let mut entries = Vec::with_capacity(playback.chapters.len());
    let mut start = None;
    for chapter in playback.chapters {
        let Some(recording) = chapter.recording_id else {
            continue;
        };
        let anchor = hito_core::AudiobookChapterAnchor {
            audiobook_id: input.audiobook_id,
            entry_id: chapter.entry_id.parse().map_err(|_| PlayerError::Invalid)?,
            recording_id: recording.parse().map_err(|_| PlayerError::Invalid)?,
        };
        if anchor == playback.chapter {
            if !chapter.available || start.is_some() {
                return Err(PlayerError::NotFound);
            }
            start = Some(entries.len());
        }
        entries.push(PlayerQueueInput {
            recording_id: anchor.recording_id,
            chapter: Some(anchor),
        });
    }
    let start = start.ok_or(PlayerError::NotFound)?;
    Ok(Prepared {
        request_id: input.id,
        fence,
        expires: Instant::now() + PREPARATION_LIFETIME,
        info: PlayerAudiobookContinuation {
            token: Uuid::new_v4(),
            chapter: playback.chapter,
            position_ms: progress.position_ms,
            speed: rate,
            count: entries.len(),
        },
        entries,
        start,
    })
}
