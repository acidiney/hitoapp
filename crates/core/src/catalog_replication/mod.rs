mod construction;
mod materialization;
mod page_encoding;
mod retention;
mod types;

use crate::{Actor, Error, HostDeviceId, Result, store::Store};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use materialization::MAX_EXPORT_BYTES;
use retention::Registry;
use sqlx::Connection;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, watch};
use tracing::{Instrument, instrument::WithSubscriber};
pub use types::{
    BeginCatalogSnapshot, CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage,
    LocalRecordingFact, LocalReleaseAppearance, LocalSourceFact, ServerRelayObservation,
};
use uuid::Uuid;

const SNAPSHOT_TTL: Duration = Duration::from_secs(300);

type Completion = Shared<BoxFuture<'static, std::result::Result<CatalogSnapshot, Arc<Error>>>>;

struct Construction {
    owner: String,
    cancel: watch::Sender<bool>,
    completion: Completion,
}

#[derive(Clone)]
pub struct CatalogReplication {
    store: Store,
    host: HostDeviceId,
    registry: Arc<Mutex<Registry>>,
    constructors: Arc<Semaphore>,
    reader: Arc<tokio::sync::Mutex<Option<sqlx::SqliteConnection>>>,
    #[cfg(test)]
    before_publication: Arc<tokio::sync::Notify>,
    #[cfg(test)]
    before_reader: Arc<tokio::sync::Notify>,
    #[cfg(test)]
    before_read_settlement: Arc<tokio::sync::Notify>,
}

impl CatalogReplication {
    pub(crate) async fn new(store: Store, host: HostDeviceId) -> Result<Self> {
        let connection =
            sqlx::SqliteConnection::connect_with(&store.pool.connect_options()).await?;
        Ok(Self {
            store,
            host,
            registry: Default::default(),
            constructors: Arc::new(Semaphore::new(1)),
            reader: Arc::new(tokio::sync::Mutex::new(Some(connection))),
            #[cfg(test)]
            before_publication: Default::default(),
            #[cfg(test)]
            before_reader: Default::default(),
            #[cfg(test)]
            before_read_settlement: Default::default(),
        })
    }

    /// Reuses this account's retained snapshot for the current library grant.
    /// Release it first when a fresh view of the library is needed.
    #[tracing::instrument(level="info",skip_all,err(Debug),fields(account_id=actor.id(),snapshot_id=tracing::field::Empty))]
    pub async fn begin(&self, actor: &Actor, library: &str) -> Result<CatalogSnapshot> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        actor.require_session()?;
        actor.require_unlocked()?;
        let permit = self
            .constructors
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        let previous = {
            self.registry
                .lock()
                .map_err(|_| unavailable())?
                .constructions
                .drain()
                .map(|(_, job)| job.completion)
                .collect::<Vec<_>>()
        };
        for completion in previous {
            let _ = completion.await;
        }
        let id = Uuid::new_v4();
        tracing::Span::current().record("snapshot_id", tracing::field::display(id));
        let (cancel, stop) = watch::channel(false);
        let completion = {
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            if registry.closed {
                return Err(unavailable());
            }
            let owner = actor.id().to_owned();
            let this = self.clone();
            let actor = actor.clone();
            let library = library.to_owned();
            let task = tokio::spawn(
                async move {
                    let _permit = permit;
                    this.construct(&actor, &library, id, deadline, stop)
                        .await
                        .map_err(Arc::new)
                }
                .instrument(tracing::Span::current())
                .with_current_subscriber(),
            );
            let completion = async move { task.await.map_err(|_| Arc::new(unavailable()))? }
                .boxed()
                .shared();
            registry.constructions.insert(
                id,
                Construction {
                    owner,
                    cancel: cancel.clone(),
                    completion: completion.clone(),
                },
            );
            completion
        };
        let _observer = retention::Observer(cancel);
        let outcome = completion.await;
        self.registry
            .lock()
            .map_err(|_| unavailable())?
            .constructions
            .remove(&id);
        outcome.map_err(|error| repeat_error(&error))
    }

    pub(super) async fn grant(
        &self,
        connection: &mut sqlx::SqliteConnection,
        actor: &Actor,
        library: &str,
    ) -> Result<String> {
        actor.require_session()?;
        actor.require_unlocked()?;
        actor.check_session_on(connection).await?;
        sqlx::query_scalar("SELECT id FROM library_access_grants WHERE user_id=? AND library_id=?")
            .bind(actor.id())
            .bind(library)
            .fetch_optional(connection)
            .await?
            .ok_or(Error::NotFound)
    }
}

fn limit() -> Error {
    Error::Conflict("Catalog snapshot exceeds its resource limit".into())
}
fn busy() -> Error {
    Error::Conflict("Catalog snapshot construction is busy or cancelled".into())
}
fn unavailable() -> Error {
    Error::Internal("Catalog snapshots are unavailable".into())
}
fn repeat_error(error: &Error) -> Error {
    match error {
        Error::Unauthorized => Error::Unauthorized,
        Error::Forbidden => Error::Forbidden,
        Error::Locked => Error::Locked,
        Error::NotFound => Error::NotFound,
        Error::Conflict(message) => Error::Conflict(message.clone()),
        Error::Upstream(message) => Error::Upstream(message),
        _ => unavailable(),
    }
}

#[cfg(test)]
#[path = "../../tests/catalog_replication/work.rs"]
mod work;

#[cfg(test)]
#[path = "../../tests/catalog_replication/deadline.rs"]
mod deadline;
