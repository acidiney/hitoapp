use crate::{
    Actor, CatalogSnapshot, CatalogSnapshotPage, Error, Result,
    replicas::{ReplicaSession, Replicas},
};
use uuid::Uuid;

impl Replicas {
    #[tracing::instrument(level="debug",skip_all,err(Debug),fields(account_id=actor.id(),connection_id=%session.connection))]
    pub async fn catalog_snapshot(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        library: &str,
    ) -> Result<Option<CatalogSnapshot>> {
        self.guarded(actor, session).await?.commit().await?;
        let profile = self.catalog_profile(session)?;
        let mut tx = profile.store.pool.begin().await?;
        let metadata:Option<String>=sqlx::query_scalar("SELECT i.metadata FROM replica_catalog_heads h JOIN replica_catalog_installs i ON i.id=h.install_id WHERE h.library_id=? AND i.account_id=? AND i.committed=1")
            .bind(library).bind(&session.remote_account).fetch_optional(&mut *tx).await?;
        let metadata = metadata
            .map(|encoded| serde_json::from_str(&encoded).map_err(|_| super::files::unavailable()))
            .transpose()?;
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        guard.commit().await?;
        Ok(metadata)
    }

    #[tracing::instrument(level="debug",skip_all,err(Debug),fields(account_id=actor.id(),connection_id=%session.connection,ordinal))]
    pub async fn catalog_page(
        &self,
        actor: &Actor,
        session: &ReplicaSession,
        snapshot: Uuid,
        ordinal: u64,
    ) -> Result<CatalogSnapshotPage> {
        self.guarded(actor, session).await?.commit().await?;
        let profile = self.catalog_profile(session)?;
        let mut tx = profile.store.pool.begin().await?;
        let (install,encoded):(String,String)=sqlx::query_as("SELECT i.id,i.metadata FROM replica_catalog_heads h JOIN replica_catalog_installs i ON i.id=h.install_id WHERE i.snapshot_id=? AND i.account_id=? AND i.committed=1")
            .bind(snapshot.to_string()).bind(&session.remote_account).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let metadata: CatalogSnapshot =
            serde_json::from_str(&encoded).map_err(|_| super::files::unavailable())?;
        if ordinal >= metadata.page_count {
            return Err(Error::NotFound);
        }
        let payloads:Vec<String>=sqlx::query_scalar("SELECT payload FROM replica_catalog_facts WHERE install_id=? AND page_ordinal=? ORDER BY position")
            .bind(install).bind(ordinal as i64).fetch_all(&mut *tx).await?;
        let recordings = payloads
            .into_iter()
            .map(|encoded| serde_json::from_str(&encoded).map_err(|_| super::files::unavailable()))
            .collect::<Result<Vec<_>>>()?;
        let guard = self.guarded(actor, session).await?;
        tx.commit().await?;
        guard.commit().await?;
        Ok(CatalogSnapshotPage {
            snapshot_id: snapshot,
            ordinal,
            recordings,
            next_ordinal: (ordinal + 1 < metadata.page_count).then_some(ordinal + 1),
        })
    }
}
