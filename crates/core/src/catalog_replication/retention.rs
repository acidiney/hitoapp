use super::{
    CatalogReplication, CatalogSnapshot, CatalogSnapshotPage, Construction, MAX_EXPORT_BYTES,
    limit, materialization::Materialized, unavailable,
};
use crate::{Actor, Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::watch, time::Instant};
use uuid::Uuid;

const MAX_RETAINED_BYTES: usize = 256 * 1024 * 1024;

pub(super) struct Retained {
    pub materialized: Materialized,
    pub grant: String,
    pub deadline: Instant,
}

#[derive(Default)]
pub(super) struct Registry {
    pub closed: bool,
    pub bytes: usize,
    pub snapshots: HashMap<Uuid, Retained>,
    pub constructions: HashMap<Uuid, Construction>,
}

impl Registry {
    pub fn resume(
        &mut self,
        account: &str,
        library: &str,
        grant: &str,
    ) -> Result<Option<CatalogSnapshot>> {
        self.expire();
        if self.closed {
            return Err(unavailable());
        }
        let existing = self.snapshots.iter().find(|(_, snapshot)| {
            snapshot.materialized.metadata.account_id == account
                && snapshot.materialized.metadata.library_id == library
        });
        match existing {
            Some((_, snapshot)) if snapshot.grant == grant => {
                Ok(Some(snapshot.materialized.metadata.clone()))
            }
            Some((id, _)) => {
                let id = *id;
                self.remove(id);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn reserve(&mut self, account: &str, library: &str) -> Result<()> {
        self.expire();
        if self.closed {
            return Err(unavailable());
        }
        if self.snapshots.len() >= 10
            || self.bytes > MAX_RETAINED_BYTES - MAX_EXPORT_BYTES
            || self.snapshots.values().any(|snapshot| {
                snapshot.materialized.metadata.account_id == account
                    && snapshot.materialized.metadata.library_id == library
            })
        {
            return Err(limit());
        }
        self.bytes += MAX_EXPORT_BYTES;
        Ok(())
    }

    pub fn remove(&mut self, id: Uuid) {
        if let Some(snapshot) = self.snapshots.remove(&id) {
            self.bytes -= snapshot.materialized.bytes;
        }
    }

    pub fn expire(&mut self) {
        let expired = self
            .snapshots
            .iter()
            .filter_map(|(id, snapshot)| (snapshot.deadline <= Instant::now()).then_some(*id))
            .collect::<Vec<_>>();
        for id in expired {
            self.remove(id);
        }
    }
}

pub(super) struct Reservation(pub Arc<Mutex<Registry>>);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .bytes -= MAX_EXPORT_BYTES;
    }
}

pub(super) struct Observer(pub watch::Sender<bool>);
impl Drop for Observer {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

impl CatalogReplication {
    /// Releases an account-owned snapshot without requiring its former library grant.
    #[tracing::instrument(level="info",skip_all,err(Debug),fields(account_id=actor.id(),snapshot_id=%id))]
    pub async fn release(&self, actor: &Actor, id: Uuid) -> Result<()> {
        let started = std::time::Instant::now();
        actor.require_session()?;
        actor.require_unlocked()?;
        if self.registry.lock().map_err(|_| unavailable())?.closed {
            return Err(unavailable());
        }
        let mut fence = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut fence).await?;
        let removed = {
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            if registry.closed {
                return Err(unavailable());
            }
            let owned = registry
                .snapshots
                .get(&id)
                .is_some_and(|snapshot| snapshot.materialized.metadata.account_id == actor.id());
            if owned {
                registry.remove(id);
            }
            owned
        };
        fence.commit().await?;
        tracing::info!(
            outcome = if removed { "released" } else { "unchanged" },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Catalog snapshot released"
        );
        Ok(())
    }

    #[tracing::instrument(level="debug",skip_all,err(Debug),fields(account_id=actor.id(),snapshot_id=%id,ordinal))]
    pub async fn page(&self, actor: &Actor, id: Uuid, ordinal: u64) -> Result<CatalogSnapshotPage> {
        let (library, expected_grant) = {
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            registry.expire();
            if registry.closed {
                return Err(unavailable());
            }
            let snapshot = registry
                .snapshots
                .get(&id)
                .filter(|snapshot| snapshot.materialized.metadata.account_id == actor.id())
                .ok_or(Error::NotFound)?;
            (
                snapshot.materialized.metadata.library_id.clone(),
                snapshot.grant.clone(),
            )
        };
        let mut fence = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        if self.grant(&mut fence, actor, &library).await? != expected_grant {
            return Err(Error::NotFound);
        }
        let page = {
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            registry.expire();
            if registry.closed {
                return Err(unavailable());
            }
            let snapshot = registry.snapshots.get(&id).ok_or(Error::NotFound)?;
            let encoded = usize::try_from(ordinal)
                .ok()
                .and_then(|index| snapshot.materialized.pages.get(index))
                .ok_or(Error::NotFound)?;
            serde_json::from_slice(encoded).map_err(|_| unavailable())?
        };
        fence.commit().await?;
        tracing::debug!(outcome = "read", "Catalog snapshot page read");
        Ok(page)
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn close(&self) -> Result<()> {
        let completions = {
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            registry.closed = true;
            registry
                .constructions
                .values()
                .map(|job| {
                    job.cancel.send_replace(true);
                    job.completion.clone()
                })
                .collect::<Vec<_>>()
        };
        for completion in completions {
            let _ = completion.await;
        }
        let closed = match self.reader.lock().await.take() {
            Some(connection) => sqlx::Connection::close(connection).await,
            None => Ok(()),
        };
        let mut registry = self.registry.lock().map_err(|_| unavailable())?;
        registry.snapshots.clear();
        registry.constructions.clear();
        registry.bytes = 0;
        tracing::info!(
            event = "catalog_snapshots_closed",
            outcome = "closed",
            "Catalog snapshots closed"
        );
        closed?;
        Ok(())
    }

    pub(crate) async fn forget_account(&self, account: &str) {
        let completions = {
            let mut registry = self
                .registry
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let ids = registry
                .snapshots
                .iter()
                .filter_map(|(id, snapshot)| {
                    (snapshot.materialized.metadata.account_id == account).then_some(*id)
                })
                .collect::<Vec<_>>();
            for id in ids {
                registry.remove(id);
            }
            registry
                .constructions
                .iter()
                .filter(|(_, job)| job.owner == account)
                .map(|(id, job)| {
                    job.cancel.send_replace(true);
                    (*id, job.completion.clone())
                })
                .collect::<Vec<_>>()
        };
        for (id, completion) in completions {
            let _ = completion.await;
            self.registry
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .constructions
                .remove(&id);
        }
        tracing::debug!(outcome = "erased", "Account catalog snapshots cleared");
    }
}
