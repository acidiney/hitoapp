use super::validation::Validation;
use crate::replicas::{
    ReplicaSession, Replicas,
    transport::{Endpoint, Remote, RemoteResult},
};
use crate::{Actor, BeginCatalogSnapshot, CatalogSnapshot, Error, Result};
use std::time::Duration;
use tokio::{sync::watch, time::Instant};
use zeroize::Zeroizing;

pub(super) struct Budget {
    stop: watch::Receiver<bool>,
    deadline: Instant,
}

impl Budget {
    pub fn check(&self) -> Result<()> {
        if *self.stop.borrow() {
            Err(Error::Conflict("Catalog installation was cancelled".into()))
        } else if Instant::now() >= self.deadline {
            Err(Error::Upstream("Catalog installation timed out"))
        } else {
            Ok(())
        }
    }
    async fn remote<T>(
        &mut self,
        replicas: &Replicas,
        actor: &Actor,
        session: &ReplicaSession,
        call: impl std::future::Future<Output = RemoteResult<T>>,
    ) -> Result<T> {
        self.check()?;
        replicas.guarded(actor, session).await?.commit().await?;
        self.check()?;
        tokio::select! {
            biased;
            _=self.stop.changed()=>Err(Error::Conflict("Catalog installation was cancelled".into())),
            _=tokio::time::sleep_until(self.deadline)=>Err(Error::Upstream("Catalog installation timed out")),
            result=replicas.remote_session_call(session,call)=>result,
        }
    }
}

impl Replicas {
    pub(super) async fn install_owned(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        request: BeginCatalogSnapshot,
        stop: watch::Receiver<bool>,
    ) -> Result<CatalogSnapshot> {
        let started = std::time::Instant::now();
        let mut budget = Budget {
            stop,
            deadline: Instant::now() + Duration::from_secs(120),
        };
        budget.check()?;
        let mut guard = self.guarded(actor, session).await?;
        let (endpoint, credential): (String, Option<Vec<u8>>) =
            sqlx::query_as("SELECT endpoint,credential FROM replica_connections WHERE id=?")
                .bind(session.connection.to_string())
                .fetch_one(&mut *guard)
                .await?;
        let credential = credential.ok_or(Error::AuthorizationRequired(
            "Sign in to this Hito server again",
        ))?;
        let token: Zeroizing<String> = self.vault.reveal_replica(
            actor.id(),
            &session.connection.to_string(),
            &endpoint,
            &session.remote_account,
            &credential,
        )?;
        guard.commit().await?;
        let remote = Remote::new(Endpoint::parse(&endpoint)?)?;
        let profile = self.catalog_profile(session)?;
        let metadata = budget
            .remote(
                self,
                actor,
                session,
                remote.begin_catalog_snapshot(&token, &session.remote_account, &request),
            )
            .await?;
        Validation::metadata(&metadata, &session.remote_account, &request.library_id)?;
        let install = uuid::Uuid::new_v4().to_string();
        self.stage_catalog(actor, session, &profile, &install, &metadata, &budget)
            .await?;
        let mut validation = Validation::default();
        for ordinal in 0..metadata.page_count {
            let (page, bytes) = budget
                .remote(
                    self,
                    actor,
                    session,
                    remote.catalog_snapshot_page(
                        &token,
                        &session.remote_account,
                        metadata.id,
                        ordinal,
                    ),
                )
                .await?;
            let payloads = validation.page(&metadata, ordinal, &page, bytes)?;
            self.stage_catalog_page(
                actor,
                session,
                &profile,
                &install,
                super::staging::Page {
                    wire: &page,
                    payloads,
                    bytes,
                },
                &budget,
            )
            .await?;
        }
        validation.complete(&metadata)?;
        self.publish_catalog(
            actor,
            session,
            &profile,
            &install,
            &metadata.library_id,
            &budget,
        )
        .await?;
        let released = budget
            .remote(
                self,
                actor,
                session,
                remote.release_catalog_snapshot(&token, &session.remote_account, metadata.id),
            )
            .await
            .is_ok();
        tracing::info!(
            outcome = "installed",
            recordings = metadata.recording_count,
            pages = metadata.page_count,
            released,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Replica catalog installed"
        );
        Ok(metadata)
    }

    pub(super) async fn settle_catalog<'a>(
        &'a self,
        actor: &Actor,
        session: &ReplicaSession,
        tx: sqlx::Transaction<'_, sqlx::Sqlite>,
        result: Result<()>,
        budget: &Budget,
    ) -> Settlement<'a> {
        let guard = match result.and_then(|()| budget.check()) {
            Ok(()) => self.guarded(actor, session).await,
            Err(error) => Err(error),
        };
        match guard {
            Ok(guard) => {
                let result = match budget.check() {
                    Ok(()) => tx.commit().await.map_err(Error::from),
                    Err(error) => tx.rollback().await.map_err(Error::from).and(Err(error)),
                };
                Settlement {
                    result,
                    guard: Some(guard),
                }
            }
            Err(error) => Settlement {
                result: tx.rollback().await.map_err(Error::from).and(Err(error)),
                guard: None,
            },
        }
    }
}

pub(super) struct Settlement<'a> {
    result: Result<()>,
    guard: Option<sqlx::Transaction<'a, sqlx::Sqlite>>,
}

impl Settlement<'_> {
    pub async fn finish(
        self,
        mut connection: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    ) -> Result<()> {
        use sqlx::{Connection, TransactionManager, sqlite::SqliteTransactionManager};
        let settled = connection.ping().await;
        let idle = SqliteTransactionManager::get_transaction_depth(&connection) == 0;
        let settled = if settled.is_err() || !idle {
            connection.close().await
        } else {
            drop(connection);
            Ok(())
        };
        if let Some(guard) = self.guard {
            guard.commit().await?;
        }
        settled?;
        self.result
    }
}
