use super::{Action, PlayerError, Request, Result, ownership::State};
use hito_core::{Recording, Services};
use std::{future::Future, pin::Pin};
use tokio::sync::{mpsc, oneshot};
use tracing::Instrument;
use uuid::Uuid;

type Admission = Pin<Box<dyn Future<Output = (Request, Result<Vec<Recording>>)> + Send>>;

pub(super) async fn run(
    services: Services,
    host: mpsc::WeakSender<Request>,
    mut requests: mpsc::Receiver<Request>,
    mut stopped: oneshot::Receiver<()>,
    mut effects: super::effects::Effects,
    media: Option<crate::MediaIntegration>,
) -> Result<()> {
    let mut media = media.map(|media| super::media::Driver::new(media, services.catalog.clone()));
    let mut state = State {
        epoch: Uuid::new_v4(),
        owner: None,
        native: effects.enabled(),
    };
    let mut pending: Option<Admission> = None;
    let mut committing: Option<super::continuation_commit::Commit> = None;
    let mut authority = super::authority::Monitor::new(&services);
    let mut progress = super::progress::Progress::new(services.collection.clone());
    let mut loader = super::loading::Loader::new(services.collection.clone());
    let mut continuation = super::continuation::Continuation::new(services.clone());
    loop {
        authority.sync(&state);
        progress.sync(&state);
        loader.sync(&state, &mut progress);
        continuation.sync(&mut state, &progress);
        if committing
            .as_ref()
            .is_some_and(|commit| !commit.current(&state))
        {
            committing.take().unwrap().retire();
        }
        effects.sync(&state, progress.can_play(&state));
        if let Some(media) = &mut media {
            media.sync(&state);
        }
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            update = effects.next() => { progress.observe(&state, &update); state.update(update); },
            report = progress.next() => {
                if let Some(owned) = &mut state.owner && owned.scope == report.scope {
                    owned.persistence_error = report.failed.then_some(PlayerError::Persistence);
                    owned.publish();
                }
            },
            command = async { media.as_mut().unwrap().next().await }, if media.is_some() => {
                if let Some(sender) = host.upgrade()
                    && sender.capacity() >= super::media::RESERVED_CLIENT_SLOTS
                    && let Some(request) = media.as_ref().unwrap().request(state.owner.as_ref().map(|owned| owned.actor.clone()), command)
                {
                    tracing::debug!(event = "media_command_dispatching", scoped_seek = matches!(command, crate::MediaCommand::SeekTrack { .. }), "Dispatching system media command");
                    if sender.try_send(request).is_err() {
                        tracing::debug!(event = "media_command_dropped", "System media command dropped while playback was busy");
                    }
                } else {
                    tracing::debug!(event = "media_command_dropped", "System media command dropped while playback was busy or the target was stale");
                }
            },
            report = loader.next() => { state.loaded(report); },
            (request, result) = continuation.next() => {
                super::continuation::Continuation::complete(&mut state, request, result);
            },
            report = authority.next() => {
                state.checked_session(report.lease, report.recovering, report.result);
            },
            (request, result) = async { pending.as_mut().unwrap().await }, if pending.is_some() => {
                pending = None;
                let span = request.span.clone();
                let _span = span.enter();
                if let Err(error) = &result { state.failed_request(&request.actor, *error); }
                if result.is_ok() && matches!(request.action, Action::PrepareAudiobook(_)) {
                    continuation.prepare(&mut state, &mut progress, request);
                    continue;
                }
                let host_command = matches!(&request.action, Action::Host { .. });
                let scoped_seek = matches!(&request.action, Action::Host { command: crate::MediaCommand::SeekTrack { .. }, .. });
                let response = result.and_then(|recordings| {
                    if let Action::CancelAudiobook { lease, id, command } = request.action {
                        if let Some(command) = command {
                            continuation.settle(&mut state, &request.actor, lease, id, command).map(super::Response::AudiobookSettlement)
                        } else {
                            continuation.cancel(&mut state, &request.actor, lease, id)?;
                            Ok(super::Response::Cancelled)
                        }
                    } else {
                        state.accept(&request.actor, request.action, recordings, progress.ready())
                    }
                });
                tracing::debug!(event = "player_request_finished", elapsed_ms = request.started.elapsed().as_millis() as u64, success = response.is_ok(), "Playback request finished");
                if host_command {
                    tracing::debug!(event = "media_command_finished", scoped_seek, success = response.is_ok(), "System media command finished");
                }
                let _ = request.reply.send(response);
            },
            request = requests.recv(), if pending.is_none() => {
                let Some(request) = request else { break; };
                let services = services.clone();
                let preflight = state.preflight(&request.actor, &request.action);
                if preflight.as_ref().is_ok_and(|selection| selection.continuation.is_some()) {
                    if committing.is_some() { let _ = request.reply.send(Err(PlayerError::Busy)); }
                    else { committing = Some(super::continuation_commit::Commit::new(request, preflight.unwrap(), services)); }
                    continue;
                }
                let span = request.span.clone();
                pending = Some(Box::pin(async move {
                    let result = async {
                        services.identity.check_session(&request.actor).await.map_err(PlayerError::from)?;
                        let selection = preflight?;
                        if selection.recordings.is_empty() { return Ok(Vec::new()); }
                        for chapters in selection.chapters.chunks(1000) {
                            services.collection.check_audiobook_chapters(&request.actor, chapters).await.map_err(PlayerError::from)?;
                        }
                        let mut selected = Vec::with_capacity(selection.recordings.len());
                        for chunk in selection.recordings.chunks(1000) {
                            selected.extend(services.catalog.recordings_by_ids(&request.actor, chunk).await.map_err(PlayerError::from)?);
                        }
                        Ok(selected)
                    }.await;
                    (request, result)
                }.instrument(span)));
            },
            result = async { committing.as_mut().unwrap().next().await }, if committing.is_some() && pending.is_none() => {
                committing.take().unwrap().complete(&mut state, progress.ready(), result);
            },
        }
    }
    requests.close();
    drop(requests);
    drop(pending);
    drop(committing);
    drop(authority);
    drop(loader);
    drop(continuation);
    state.owner = None;
    if let Some(media) = &mut media {
        media.close();
    }
    let (_, saved) = tokio::join!(effects.close(), progress.close());
    tracing::info!(event = "player_owner_closed", "Playback admission joined");
    saved
}
