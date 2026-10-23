use crate::{
    Error, Snapshot, Source, Status,
    session_worker::{self, OwnerStop, Request},
};
use hito_core::PlaybackSource;
use std::sync::{Arc, mpsc};
use tokio::sync::{oneshot, watch};

pub(crate) async fn own(
    grant: PlaybackSource,
    requests: mpsc::Receiver<Request>,
    cancel: Arc<OwnerStop>,
    state: watch::Sender<Snapshot>,
    mut stopped: oneshot::Receiver<()>,
) {
    let input_stop = cancel.input();
    let input = tokio::select! {
        biased;
        _ = &mut stopped => { finish(&state, Status::Stopped, None); return; },
        _ = input_stop.cancelled() => { finish(&state, Status::Stopped, None); return; },
        _ = grant.revoked() => { finish(&state, Status::Revoked, Some(Error::Access)); return; },
        result = Source::open_with_stop(grant.clone(), cancel.input()) => match result {
            Ok(input) => input,
            Err(_) => {
                if grant.check().await.is_err() { finish(&state, Status::Revoked, Some(Error::Access)); }
                else { finish(&state, Status::Failed, Some(Error::Input)); }
                return;
            },
        },
    };
    state.send_modify(|state| state.status = Status::Starting);
    let worker_stop = cancel.clone();
    let worker_state = state.clone();
    let (started, mut opening) = oneshot::channel();
    let (activate, activation) = mpsc::sync_channel(1);
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let mut worker = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _entered = span.enter();
            session_worker::run(
                input,
                requests,
                &worker_stop,
                worker_state,
                started,
                activation,
            )
        })
    });
    let mut started = false;
    let (status, error, joined) = loop {
        tokio::select! {
            biased;
            _ = &mut stopped => break (Status::Stopped, None, false),
            _ = input_stop.cancelled() => break (Status::Stopped, None, false),
            _ = grant.revoked() => break (Status::Revoked, Some(Error::Access), false),
            result = &mut worker => {
                if grant.check().await.is_err() { break (Status::Revoked, Some(Error::Access), true); }
                break match result {
                    Ok(Ok(())) => (Status::Stopped, None, true),
                    Ok(Err(error)) => (Status::Failed, Some(error), true),
                    Err(_) => (Status::Failed, Some(Error::Worker), true),
                };
            },
            result = &mut opening, if !started => {
                if grant.check().await.is_err() { break (Status::Revoked, Some(Error::Access), false); }
                if let Err(error) = result.unwrap_or(Err(Error::Worker)) { break (Status::Failed, Some(error), false); }
                if cancel.requested() { break (Status::Stopped, None, false); }
                if activate.send(()).is_err() { break (Status::Failed, Some(Error::Worker), false); }
                started = true;
                state.send_modify(|state| state.status = Status::Ready);
            },
        }
    };
    cancel.request();
    if !joined {
        let _ = worker.await;
    }
    finish(&state, status, error);
}

fn finish(state: &watch::Sender<Snapshot>, status: Status, error: Option<Error>) {
    let mut final_state = *state.borrow();
    final_state.status = status;
    final_state.playing = false;
    final_state.error = error;
    session_worker::publish(state, final_state);
    tracing::info!(
        event = "native_session_finished",
        ?status,
        ?error,
        "Native playback resources reclaimed"
    );
}
