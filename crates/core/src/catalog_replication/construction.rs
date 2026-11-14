use super::{
    CatalogReplication, CatalogSnapshot, SNAPSHOT_TTL, busy, limit,
    materialization::{self, Materialized},
    retention::{Reservation, Retained},
    unavailable,
};
use crate::{Actor, Error, Result};
use sqlx::{Connection, SqliteConnection};
use tokio::{sync::watch, time::Instant};
use uuid::Uuid;

enum Prepared {
    Resume(CatalogSnapshot),
    Fresh(Materialized),
}

impl Prepared {
    fn metadata(&self) -> &CatalogSnapshot {
        match self {
            Self::Resume(metadata) => metadata,
            Self::Fresh(materialized) => &materialized.metadata,
        }
    }
}

struct Budget {
    deadline: Instant,
    stop: watch::Receiver<bool>,
    retention_started: Instant,
}

impl Budget {
    fn check(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            Err(timed_out())
        } else if *self.stop.borrow() {
            Err(busy())
        } else {
            Ok(())
        }
    }
}

fn timed_out() -> Error {
    Error::Upstream("Catalog snapshot construction timed out")
}

impl CatalogReplication {
    pub(super) async fn construct(
        &self,
        actor: &Actor,
        library: &str,
        id: Uuid,
        deadline: Instant,
        stop: watch::Receiver<bool>,
    ) -> Result<CatalogSnapshot> {
        let started = std::time::Instant::now();
        let mut budget = Budget {
            deadline,
            stop,
            retention_started: Instant::now(),
        };
        budget.check()?;
        #[cfg(test)]
        self.before_reader.notify_one();
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "reader_wait");
        let mut idle = tokio::select! {
            biased;
            _=tokio::time::sleep_until(deadline)=>return Err(timed_out()),
            _=budget.stop.changed()=>return Err(busy()),
            slot=self.reader.lock()=>slot,
        };
        budget.check()?;
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "reader_open");
        let mut connection = match idle.take() {
            Some(connection) => connection,
            None => SqliteConnection::connect_with(&self.store.pool.connect_options()).await?,
        };
        let mut reservation: Option<Reservation> = None;
        let outcome = async {
            budget.check()?;
            let (prepared, grant) = self
                .read(
                    &mut connection,
                    actor,
                    library,
                    id,
                    &mut budget,
                    &mut reservation,
                )
                .await?;
            let resumed = matches!(prepared, Prepared::Resume(_));
            let snapshot = self
                .publish(&mut connection, actor, &grant, prepared, &budget)
                .await?;
            tracing::Span::current().record("snapshot_id", tracing::field::display(snapshot.id));
            if resumed {
                tracing::debug!(
                    outcome = "resumed",
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Catalog snapshot resumed"
                );
            } else {
                tracing::info!(
                    recordings = snapshot.recording_count,
                    pages = snapshot.page_count,
                    outcome = "ready",
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Catalog snapshot constructed"
                );
            }
            Ok(snapshot)
        }
        .await;
        match outcome {
            Ok(snapshot) => {
                *idle = Some(connection);
                Ok(snapshot)
            }
            Err(error) => {
                #[cfg(test)]
                tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "reader_close");
                connection.close().await?;
                Err(error)
            }
        }
    }

    async fn read(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        library: &str,
        id: Uuid,
        budget: &mut Budget,
        reservation: &mut Option<Reservation>,
    ) -> Result<(Prepared, String)> {
        budget.check()?;
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "read_transaction");
        let mut tx = connection.begin().await?;
        let result = tokio::select! {
            biased;
            _=tokio::time::sleep_until(budget.deadline)=>Err(timed_out()),
            _=budget.stop.changed()=>Err(busy()),
            result=async {
                #[cfg(test)]
                tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "read_authority");
                let grant=self.grant(&mut tx,actor,library).await?;
                {
                    let mut registry = self.registry.lock().map_err(|_| unavailable())?;
                    if let Some(snapshot) = registry.resume(actor.id(), library, &grant)? {
                        return Ok((Prepared::Resume(snapshot), grant));
                    }
                    registry.reserve(actor.id(),library)?;
                }
                *reservation=Some(Reservation(self.registry.clone()));
                let materialized=materialization::collect(&mut tx,actor,&self.host,library,id).await?;
                Ok((Prepared::Fresh(materialized),grant))
            }=>result,
        };
        #[cfg(test)]
        if result.is_err() {
            self.before_read_settlement.notify_one();
        }
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "read_settlement");
        let settled = if result.is_ok() {
            tx.commit().await
        } else {
            tx.rollback().await
        };
        settled?;
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "read_settled");
        budget.check()?;
        result
    }

    async fn publish(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        expected_grant: &str,
        prepared: Prepared,
        budget: &Budget,
    ) -> Result<CatalogSnapshot> {
        budget.check()?;
        let metadata = prepared.metadata().clone();
        let id = metadata.id;
        let library = &metadata.library_id;
        let fresh = matches!(prepared, Prepared::Fresh(_));
        #[cfg(test)]
        self.before_publication.notify_one();
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "publication_transaction");
        let mut fence = connection.begin_with("BEGIN IMMEDIATE").await?;
        let publication: Result<()> = async {
            budget.check()?;
            #[cfg(test)]
            tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "publication_authority");
            if self.grant(&mut fence, actor, library).await? != expected_grant {
                return Err(Error::NotFound);
            }
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            budget.check()?;
            if registry.closed {
                return Err(unavailable());
            }
            match prepared {
                Prepared::Resume(_) => {
                    if registry
                        .resume(actor.id(), library, expected_grant)?
                        .as_ref()
                        != Some(&metadata)
                    {
                        return Err(Error::NotFound);
                    }
                }
                Prepared::Fresh(materialized) => {
                    if materialized.bytes > super::MAX_EXPORT_BYTES {
                        return Err(limit());
                    }
                    registry.bytes += materialized.bytes;
                    registry.snapshots.insert(
                        id,
                        Retained {
                            materialized,
                            grant: expected_grant.to_owned(),
                            deadline: budget.retention_started + SNAPSHOT_TTL,
                        },
                    );
                }
            }
            Ok(())
        }
        .await;
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "publication_settlement");
        let completed = if publication.is_ok() {
            fence.commit().await
        } else {
            fence.rollback().await
        };
        let valid = budget.check();
        if fresh && (publication.is_err() || completed.is_err() || valid.is_err()) {
            self.registry.lock().map_err(|_| unavailable())?.remove(id);
        }
        completed?;
        publication?;
        valid?;
        #[cfg(test)]
        tracing::trace!(target: "hito_core::catalog_snapshot_stage_test", stage = "ready");
        Ok(metadata)
    }
}
