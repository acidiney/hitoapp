use super::{
    CatalogReplication, CatalogSnapshot, SNAPSHOT_TTL, busy, limit,
    materialization::{self, Materialized},
    retention::Retained,
    unavailable,
};
use crate::{Actor, Error, Result};
use sqlx::{Connection, SqliteConnection};
use tokio::{sync::watch, time::Instant};
use uuid::Uuid;

struct Budget {
    deadline: Instant,
    stop: watch::Receiver<bool>,
    started: std::time::Instant,
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
            started,
        };
        budget.check()?;
        #[cfg(test)]
        self.before_reader.notify_one();
        let mut idle = tokio::select! {
            biased;
            _=tokio::time::sleep_until(deadline)=>return Err(timed_out()),
            _=budget.stop.changed()=>return Err(busy()),
            slot=self.reader.lock()=>slot,
        };
        budget.check()?;
        let mut connection = match idle.take() {
            Some(connection) => connection,
            None => SqliteConnection::connect_with(&self.store.pool.connect_options()).await?,
        };
        let outcome = async {
            budget.check()?;
            let (materialized, grant) = self
                .read(&mut connection, actor, library, id, &mut budget)
                .await?;
            self.publish(&mut connection, actor, &grant, materialized, &budget)
                .await
        }
        .await;
        match outcome {
            Ok(snapshot) => {
                *idle = Some(connection);
                tracing::info!(snapshot_id=%id,recordings=snapshot.recording_count,pages=snapshot.page_count,outcome="ready",elapsed_ms=started.elapsed().as_millis() as u64,"Catalog snapshot constructed");
                Ok(snapshot)
            }
            Err(error) => {
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
    ) -> Result<(Materialized, String)> {
        budget.check()?;
        let mut tx = connection.begin().await?;
        let result = tokio::select! {
            biased;
            _=tokio::time::sleep_until(budget.deadline)=>Err(timed_out()),
            _=budget.stop.changed()=>Err(busy()),
            result=async {
                let grant=self.grant(&mut tx,actor,library).await?;
                let materialized=materialization::collect(&mut tx,actor,&self.host,library,id).await?;
                Ok((materialized,grant))
            }=>result,
        };
        let settled = if result.is_ok() {
            tx.commit().await
        } else {
            tx.rollback().await
        };
        settled?;
        budget.check()?;
        result
    }

    async fn publish(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        expected_grant: &str,
        materialized: Materialized,
        budget: &Budget,
    ) -> Result<CatalogSnapshot> {
        budget.check()?;
        let id = materialized.metadata.id;
        let library = materialized.metadata.library_id.clone();
        #[cfg(test)]
        self.before_publication.notify_one();
        let mut fence = connection.begin_with("BEGIN IMMEDIATE").await?;
        let metadata = materialized.metadata.clone();
        let publication: Result<()> = async {
            budget.check()?;
            if self.grant(&mut fence, actor, &library).await? != expected_grant {
                return Err(Error::NotFound);
            }
            let mut registry = self.registry.lock().map_err(|_| unavailable())?;
            budget.check()?;
            if registry.closed {
                return Err(unavailable());
            }
            if materialized.bytes > super::MAX_EXPORT_BYTES {
                return Err(limit());
            }
            registry.bytes += materialized.bytes;
            registry.snapshots.insert(
                id,
                Retained {
                    materialized,
                    grant: expected_grant.to_owned(),
                    deadline: Instant::from_std(budget.started) + SNAPSHOT_TTL,
                },
            );
            Ok(())
        }
        .await;
        let completed = if publication.is_ok() {
            fence.commit().await
        } else {
            fence.rollback().await
        };
        let valid = budget.check();
        if publication.is_err() || completed.is_err() || valid.is_err() {
            self.registry.lock().map_err(|_| unavailable())?.remove(id);
        }
        completed?;
        publication?;
        valid?;
        Ok(metadata)
    }
}
