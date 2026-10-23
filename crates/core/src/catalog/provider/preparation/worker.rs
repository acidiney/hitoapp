use super::{Catalog, lease::Lease, publication::Completion};
use crate::{
    Error, Result,
    plugins::{PreparationExchange, PreparationOperation},
};
use std::time::{Duration, Instant};
use tokio::sync::watch;

impl Catalog {
    pub async fn run_next_source_preparation_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let (stop, stopped) = watch::channel(false);
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let job = async move {
            let result = self.run_source_preparation(stopped).await;
            drop(done);
            result
        };
        let cancellation = async move {
            tokio::select! {
                _ = shutdown => { stop.send_replace(true); },
                _ = finished => {},
            }
        };
        let (result, ()) = tokio::join!(job, cancellation);
        result
    }

    async fn run_source_preparation(&self, mut stopped: watch::Receiver<bool>) -> Result<bool> {
        if *stopped.borrow() {
            return Ok(false);
        }
        let Some(lease) = self.claim_source_preparation().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let (completion, fenced) = self.exchange_source_preparation(&lease, &mut stopped).await;
        let outcome = self
            .finish_source_preparation(&lease, completion, fenced)
            .await?;
        tracing::info!(event = "source_preparation_attempt_finished", preparation_id = %lease.id,
            account_id = %lease.user_id, outcome, elapsed_ms = started.elapsed().as_millis() as u64,
            "Source preparation attempt finished");
        Ok(true)
    }

    async fn exchange_source_preparation(
        &self,
        lease: &Lease,
        stopped: &mut watch::Receiver<bool>,
    ) -> (Completion, bool) {
        if *stopped.borrow() {
            return (Completion::NotStarted(None), false);
        }
        let operation = match tokio::time::timeout(
            Duration::from_secs(1),
            self.plugins.preparation_operation(&lease.id),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Source preparation authorization could not be checked in time",
        ))) {
            Ok(operation) => operation,
            Err(error) => return (Completion::NotStarted(Some(error)), false),
        };
        if *stopped.borrow() {
            return (Completion::NotStarted(None), false);
        }
        let authorized = if operation.submits() {
            lease.mark_submitting(&self.store).await
        } else {
            match self.store.pool.acquire().await {
                Ok(mut connection) => lease.check_on(&mut connection).await,
                Err(error) => Err(error.into()),
            }
        };
        if let Err(error) = authorized {
            return (Completion::NotStarted(Some(error)), false);
        }
        self.receive_source_preparation(lease, &operation, stopped)
            .await
    }

    async fn receive_source_preparation(
        &self,
        lease: &Lease,
        operation: &PreparationOperation,
        stopped: &mut watch::Receiver<bool>,
    ) -> (Completion, bool) {
        let submits = operation.submits();
        let mut draining = *stopped.borrow();
        if draining && !submits {
            return (Completion::Polled(Ok(None)), false);
        }
        let receiving = operation.exchange();
        tokio::pin!(receiving);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut fenced = false;
        loop {
            tokio::select! {
                biased;
                result = &mut receiving => {
                    let completion = match (submits, result) {
                        (true, Ok(PreparationExchange::Submission(submission))) => Completion::Submitted(Ok(submission)),
                        (false, Ok(PreparationExchange::Status(status))) => Completion::Polled(Ok(Some(status))),
                        (true, Err(error)) => Completion::Submitted(Err(error)),
                        (false, Err(error)) => Completion::Polled(Err(error)),
                        (true, _) => Completion::Submitted(Err(Error::Upstream("The provider returned an invalid preparation acknowledgement"))),
                        (false, _) => Completion::Polled(Err(Error::Upstream("The provider returned an invalid preparation status"))),
                    };
                    return (completion, fenced);
                },
                _ = stopped.changed(), if !draining => {
                    draining = true;
                    if !submits {
                        return (Completion::Polled(Ok(None)), fenced);
                    }
                },
                _ = heartbeat.tick() => {
                    match lease.heartbeat(&self.store).await {
                        Ok(current) if current.owned && current.authorized => {},
                        _ => {
                            fenced = true;
                            if !submits {
                                return (Completion::Polled(Ok(None)), true);
                            }
                        },
                    }
                },
            }
        }
    }
}
