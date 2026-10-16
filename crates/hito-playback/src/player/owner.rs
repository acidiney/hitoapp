use super::{PlayerError, Request, Result, ownership::State};
use hito_core::{Recording, Services};
use std::{future::Future, pin::Pin};
use tokio::sync::{mpsc, oneshot};
use tracing::Instrument;
use uuid::Uuid;

type Admission = Pin<Box<dyn Future<Output = (Request, Result<Vec<Recording>>)> + Send>>;
type SessionCheck = Pin<Box<dyn Future<Output = (Uuid, Result<()>)> + Send>>;

pub(super) async fn run(
    services: Services,
    mut requests: mpsc::Receiver<Request>,
    mut stopped: oneshot::Receiver<()>,
    mut effects: super::effects::Effects,
) {
    let mut state = State {
        epoch: Uuid::new_v4(),
        owner: None,
        native: effects.enabled(),
    };
    let mut pending: Option<Admission> = None;
    let mut checking: Option<SessionCheck> = None;
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        effects.sync(&state);
        tokio::select! {
            biased;
            _ = &mut stopped => break,
            update = effects.next() => state.update(update),
            (lease, result) = async { checking.as_mut().unwrap().await }, if checking.is_some() => {
                checking = None;
                if result.is_err() && state.owner.as_ref().is_some_and(|owned| owned.lease == lease) {
                    state.owner = None;
                    state.epoch = Uuid::new_v4();
                    tracing::info!(event = "player_session_withdrawn", "Playback ownership cleared");
                }
            },
            (request, result) = async { pending.as_mut().unwrap().await }, if pending.is_some() => {
                pending = None;
                let _span = request.span.enter();
                let response = result.and_then(|recordings| state.accept(&request.actor, request.action, recordings));
                tracing::debug!(event = "player_request_finished", elapsed_ms = request.started.elapsed().as_millis() as u64, success = response.is_ok(), "Playback request finished");
                let _ = request.reply.send(response);
            },
            _ = interval.tick(), if checking.is_none() => {
                if let Some(owned) = &state.owner {
                    let identity = services.identity.clone();
                    let actor = owned.actor.clone();
                    let lease = owned.lease;
                    checking = Some(Box::pin(async move {
                        (lease, identity.check_session(&actor).await.map_err(PlayerError::from))
                    }));
                }
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
                        if recordings.is_empty() { Ok(Vec::new()) }
                        else { services.catalog.recordings_by_ids(&request.actor, &recordings).await.map_err(PlayerError::from) }
                    }.await;
                    (request, result)
                }.instrument(span)));
            },
        }
    }
    requests.close();
    drop(requests);
    drop(pending);
    drop(checking);
    state.owner = None;
    effects.close().await;
    tracing::info!(event = "player_owner_closed", "Playback admission joined");
}
