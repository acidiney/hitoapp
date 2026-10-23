use super::{
    Action, PlayerAction, PlayerError, Request, Result,
    ownership::{AdmissionSelection, State},
};
use hito_core::{AudiobookPlaybackAdmission, Services};
use std::{future::Future, pin::Pin};
use tracing::Instrument;

type Admission = Pin<Box<dyn Future<Output = Result<AudiobookPlaybackAdmission>> + Send>>;

pub(super) struct Commit {
    request: Request,
    admission: Admission,
}
impl Commit {
    pub fn new(request: Request, selection: AdmissionSelection, services: Services) -> Self {
        let actor = request.actor.clone();
        let target = selection.continuation.unwrap();
        let span = request.span.clone();
        Self {
            request,
            admission: Box::pin(
                async move {
                    services
                        .collection
                        .admit_audiobook_playback(&actor, target, &selection.chapters)
                        .await
                        .map_err(PlayerError::from)
                }
                .instrument(span),
            ),
        }
    }
    pub fn current(&self, state: &State) -> bool {
        let Action::Submit(command) = &self.request.action else {
            return false;
        };
        let PlayerAction::ContinueAudiobook { token } = command.action else {
            return false;
        };
        !self.request.reply.is_closed()
            && state
                .owned(&self.request.actor, command.lease)
                .is_ok_and(|owned| {
                    owned.check(command).is_ok_and(|receipt| receipt.is_none())
                        && owned
                            .continuation
                            .as_ref()
                            .is_some_and(|prepared| prepared.check(owned, token).is_ok())
                })
    }
    pub async fn next(&mut self) -> Result<AudiobookPlaybackAdmission> {
        tokio::select! {
            biased;
            _ = self.request.reply.closed() => Err(PlayerError::Conflict),
            result = &mut self.admission => result,
        }
    }
    pub fn complete(
        self,
        state: &mut State,
        progress_ready: bool,
        result: Result<AudiobookPlaybackAdmission>,
    ) {
        let span = self.request.span.clone();
        let _entered = span.enter();
        let response = if !self.current(state) {
            Err(PlayerError::Conflict)
        } else {
            result.and_then(|admission| {
                admission
                    .publish(|| {
                        state.accept(
                            &self.request.actor,
                            self.request.action,
                            Vec::new(),
                            progress_ready,
                        )
                    })
                    .map_err(PlayerError::from)
                    .and_then(|result| result)
            })
        };
        tracing::debug!(
            event = "player_audiobook_commit_finished",
            success = response.is_ok(),
            elapsed_ms = self.request.started.elapsed().as_millis() as u64,
            "Audiobook continuation admission finished"
        );
        let _ = self.request.reply.send(response);
    }
    pub fn retire(self) {
        let _ = self.request.reply.send(Err(PlayerError::Conflict));
    }
}
