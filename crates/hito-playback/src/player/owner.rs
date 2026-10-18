use super::{Action, PlayerAction, PlayerError, Request, Result, ownership::State};
use hito_core::{Recording, Services};
use std::{future::Future, pin::Pin};
use tokio::sync::{mpsc, oneshot};
use tracing::Instrument;
use uuid::Uuid;

type Admission = Pin<Box<dyn Future<Output = (Request, Result<Vec<Recording>>)> + Send>>;

pub(super) async fn run(
    services: Services,
    mut requests: mpsc::Receiver<Request>,
    mut stopped: oneshot::Receiver<()>,
    mut effects: super::effects::Effects,
) -> Result<()> {
    let mut state = State {
        epoch: Uuid::new_v4(),
        owner: None,
        native: effects.enabled(),
    };
    let mut pending: Option<Admission> = None;
    let mut authority = super::authority::Monitor::new(&services);
    let mut progress = super::progress::Progress::new(services.collection.clone());
    let mut loader = super::loading::Loader::new(services.collection.clone());
    loop {
        authority.sync(&state);
        progress.sync(&state);
        loader.sync(&state, &mut progress);
        effects.sync(&state, progress.can_play(&state));
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
            report = loader.next() => { state.loaded(report); },
            report = authority.next() => {
                state.checked_session(report.lease, report.recovering, report.result);
            },
            (request, result) = async { pending.as_mut().unwrap().await }, if pending.is_some() => {
                pending = None;
                let _span = request.span.enter();
                if let Err(error) = &result { state.failed_request(&request.actor, *error); }
                let response = result.and_then(|recordings| state.accept(&request.actor, request.action, recordings, progress.ready()));
                tracing::debug!(event = "player_request_finished", elapsed_ms = request.started.elapsed().as_millis() as u64, success = response.is_ok(), "Playback request finished");
                let _ = request.reply.send(response);
            },
            request = requests.recv(), if pending.is_none() => {
                let Some(request) = request else { break; };
                let services = services.clone();
                let preflight = state.preflight(&request.actor, &request.action);
                let span = request.span.clone();
                pending = Some(Box::pin(async move {
                    let result = async {
                        services.identity.check_session(&request.actor).await.map_err(PlayerError::from)?;
                        let recordings = preflight?;
                        if recordings.is_empty() { return Ok(Vec::new()); }
                        if let Action::Submit(command) = &request.action {
                            match &command.action {
                                PlayerAction::ReplaceEntries { entries, .. }
                                | PlayerAction::AppendEntries { entries } => {
                                    let chapters = entries.iter().filter_map(|entry| entry.chapter).collect::<Vec<_>>();
                                    if !chapters.is_empty() {
                                        services.collection.check_audiobook_chapters(&request.actor, &chapters).await.map_err(PlayerError::from)?;
                                    }
                                }
                                _ => (),
                            }
                        }
                        services.catalog.recordings_by_ids(&request.actor, &recordings).await.map_err(PlayerError::from)
                    }.await;
                    (request, result)
                }.instrument(span)));
            },
        }
    }
    requests.close();
    drop(requests);
    drop(pending);
    drop(authority);
    drop(loader);
    state.owner = None;
    let (_, saved) = tokio::join!(effects.close(), progress.close());
    tracing::info!(event = "player_owner_closed", "Playback admission joined");
    saved
}
