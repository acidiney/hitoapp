mod installation;
mod reading;
pub(super) mod staging;
mod validation;

use super::{ReplicaSession, Replicas, files, profile::Profile};
use crate::{Actor, BeginCatalogSnapshot, CatalogSnapshot, Error, Result};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;

type Completion = Shared<BoxFuture<'static, std::result::Result<CatalogSnapshot, Arc<Error>>>>;

pub(super) struct Work {
    cancel: watch::Sender<bool>,
    completion: Completion,
}

struct Observer(watch::Sender<bool>);
impl Drop for Observer {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl Replicas {
    #[tracing::instrument(level="info",skip_all,err(Debug),fields(account_id=actor.id(),connection_id=%session.connection,request_id=%Uuid::new_v4()))]
    pub async fn install_catalog(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        request: BeginCatalogSnapshot,
    ) -> Result<CatalogSnapshot> {
        self.guarded(actor, session).await?.commit().await?;
        let binding = self
            .profiles
            .synchronizing
            .enter(&session.connection.to_string())
            .map_err(|_| Error::Conflict("This replica is already synchronizing".into()))?;
        let network = self
            .profiles
            .network
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Replica requests are busy. Try again.".into()))?;
        let id = Uuid::new_v4();
        let (cancel, stop) = watch::channel(false);
        let completion = {
            let mut work = self
                .profiles
                .catalog
                .lock()
                .map_err(|_| files::unavailable())?;
            self.ensure_open()?;
            work.retain(|_, work| work.completion.peek().is_none());
            let this = self.clone();
            let actor = actor.clone();
            let session = session.clone();
            let task = tokio::spawn(
                async move {
                    let _binding = binding;
                    let _network = network;
                    let started = std::time::Instant::now();
                    let result = this
                        .install_owned(&actor, &session, request, stop)
                        .await
                        .map_err(Arc::new);
                    if let Err(error) = &result {
                        tracing::info!(
                            outcome = "installation_failed",
                            error_code = error.code(),
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "Replica catalog installation ended"
                        );
                    }
                    if let Ok(mut work) = this.profiles.catalog.lock() {
                        work.remove(&id);
                    }
                    result
                }
                .instrument(tracing::Span::current())
                .with_current_subscriber(),
            );
            let completion = async move { task.await.map_err(|_| Arc::new(files::unavailable()))? }
                .boxed()
                .shared();
            work.insert(
                id,
                Work {
                    cancel: cancel.clone(),
                    completion: completion.clone(),
                },
            );
            completion
        };
        let _observer = Observer(cancel);
        let result = completion.await;
        self.profiles
            .catalog
            .lock()
            .map_err(|_| files::unavailable())?
            .remove(&id);
        result.map_err(|error| repeat_error(&error))
    }

    pub(super) fn catalog_profile(&self, session: &ReplicaSession) -> Result<Arc<Profile>> {
        self.profiles
            .active
            .lock()
            .map_err(|_| files::unavailable())?
            .get(&session.connection)
            .cloned()
            .ok_or(Error::NotFound)
    }

    pub(super) async fn close_catalog(&self) -> Result<()> {
        let work = self
            .profiles
            .catalog
            .lock()
            .map_err(|_| files::unavailable())?
            .drain()
            .map(|(_, work)| work)
            .collect::<Vec<_>>();
        for work in &work {
            work.cancel.send_replace(true);
        }
        for work in work {
            let _ = work.completion.await;
        }
        Ok(())
    }
}

fn repeat_error(error: &Error) -> Error {
    match error {
        Error::Unauthorized => Error::Unauthorized,
        Error::Forbidden => Error::Forbidden,
        Error::Locked => Error::Locked,
        Error::NotFound => Error::NotFound,
        Error::Conflict(value) => Error::Conflict(value.clone()),
        Error::Invalid(value) => Error::Invalid(value.clone()),
        Error::OperationRejected(value) => Error::OperationRejected(value.clone()),
        Error::AuthorizationRequired(value) => Error::AuthorizationRequired(value),
        Error::Upstream(value) => Error::Upstream(value),
        Error::RateLimited(value) => Error::RateLimited(*value),
        _ => files::unavailable(),
    }
}

#[cfg(test)]
#[path = "../../../tests/replicas/catalog_settlement.rs"]
mod settlement;
