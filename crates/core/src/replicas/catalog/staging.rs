use super::{installation::Budget, validation::MAX_PROFILE_BYTES};
use crate::{
    Actor, CatalogSnapshot, CatalogSnapshotPage, Error, Result,
    replicas::{ReplicaSession, Replicas, profile::Profile},
};
use sqlx::{Connection, Sqlite, Transaction};

pub(in crate::replicas) async fn prune(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query("DELETE FROM replica_catalog_facts WHERE install_id NOT IN (SELECT install_id FROM replica_catalog_heads)").execute(&mut **tx).await?;
    sqlx::query("DELETE FROM replica_catalog_installs WHERE id NOT IN (SELECT install_id FROM replica_catalog_heads)").execute(&mut **tx).await?;
    Ok(())
}

pub(super) struct Page<'a> {
    pub wire: &'a CatalogSnapshotPage,
    pub payloads: Vec<String>,
    pub bytes: usize,
}

impl Replicas {
    pub(super) async fn stage_catalog(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        profile: &Profile,
        install: &str,
        metadata: &CatalogSnapshot,
        budget: &Budget,
    ) -> Result<()> {
        budget.check()?;
        let mut connection = profile.store.pool.acquire().await?;
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result=async {
            prune(&mut tx).await?;
            let aliases: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM replica_catalog_installs i JOIN replica_catalog_heads h ON h.install_id=i.id WHERE i.snapshot_id=? AND i.library_id<>?)")
                .bind(metadata.id.to_string()).bind(&metadata.library_id).fetch_one(&mut *tx).await?;
            if aliases {return Err(super::validation::invalid());}
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM replica_catalog_heads WHERE library_id<>?").bind(&metadata.library_id).fetch_one(&mut *tx).await?;
            if count>=16 {return Err(Error::Conflict("The replica catalog library limit has been reached".into()));}
            let encoded=serde_json::to_string(metadata).map_err(|_| super::validation::invalid())?;
            sqlx::query("INSERT INTO replica_catalog_installs(id,account_id,library_id,snapshot_id,metadata) VALUES(?,?,?,?,?)")
                .bind(install).bind(&session.remote_account).bind(&metadata.library_id).bind(metadata.id.to_string()).bind(encoded).execute(&mut *tx).await?;
            Ok(())
        }.await;
        self.settle_catalog(actor, session, tx, result, budget)
            .await
            .finish(connection)
            .await
    }

    pub(super) async fn stage_catalog_page(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        profile: &Profile,
        install: &str,
        page: Page<'_>,
        budget: &Budget,
    ) -> Result<()> {
        budget.check()?;
        let mut connection = profile.store.pool.acquire().await?;
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result=async {
            let retained:i64=sqlx::query_scalar("SELECT COALESCE(sum(bytes),0) FROM replica_catalog_installs").fetch_one(&mut *tx).await?;
            let bytes=i64::try_from(page.bytes).map_err(|_|super::validation::invalid())?;
            if retained.checked_add(bytes).is_none_or(|total|total>MAX_PROFILE_BYTES) {return Err(Error::Conflict("The replica catalog storage limit has been reached".into()));}
            for (position,(fact,payload)) in page.wire.recordings.iter().zip(page.payloads).enumerate() {
                sqlx::query("INSERT INTO replica_catalog_facts(install_id,id,page_ordinal,position,payload) VALUES(?,?,?,?,?)")
                    .bind(install).bind(&fact.id).bind(page.wire.ordinal as i64).bind(position as i64).bind(payload).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE replica_catalog_installs SET bytes=bytes+? WHERE id=? AND committed=0").bind(bytes).bind(install).execute(&mut *tx).await?;
            Ok(())
        }.await;
        self.settle_catalog(actor, session, tx, result, budget)
            .await
            .finish(connection)
            .await
    }

    pub(super) async fn publish_catalog(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        profile: &Profile,
        install: &str,
        library: &str,
        budget: &Budget,
    ) -> Result<()> {
        budget.check()?;
        let mut connection = profile.store.pool.acquire().await?;
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result=async {
            sqlx::query("UPDATE replica_catalog_installs SET committed=1 WHERE id=?").bind(install).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO replica_catalog_heads(library_id,install_id) VALUES(?,?) ON CONFLICT(library_id) DO UPDATE SET install_id=excluded.install_id")
                .bind(library).bind(install).execute(&mut *tx).await?;
            Ok(())
        }.await;
        self.settle_catalog(actor, session, tx, result, budget)
            .await
            .finish(connection)
            .await
    }
}
