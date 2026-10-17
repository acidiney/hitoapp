use super::{
    Catalog, PrepareSource, SourcePreparationReceipt, SourcePreparationStatus, authority,
    types::{StatusRow, identity},
};
use crate::{Actor, Error, Result, identity::recording_access, plugins::PreparationConnection};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

const MAX_ACCOUNT_PENDING: i64 = 16;
const MAX_PENDING: i64 = 128;
const MAX_ACCOUNT_RECEIPTS: i64 = 256;
const MAX_RECEIPTS: i64 = 4096;

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn prepare_source(
        &self,
        actor: &Actor,
        recording: &str,
        source: &str,
        request: PrepareSource,
    ) -> Result<SourcePreparationReceipt> {
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        if request.request_id.is_nil() {
            return Err(Error::Invalid(
                "Use a nonempty preparation request identity".into(),
            ));
        }
        let digest = fingerprint(recording, source)?;
        let (preparation, outcome) = match replay(&mut tx, actor.id(), request.request_id, &digest)
            .await?
        {
            Some(preparation) => {
                authority::cancel_stale_on(&mut tx, &preparation).await?;
                (preparation, "replayed")
            }
            None => {
                receipt_capacity(&mut tx, actor.id()).await?;
                if let Some(previous) = latest(&mut tx, actor.id(), recording, source).await? {
                    authority::cancel_stale_on(&mut tx, &previous).await?;
                }
                let retained: Option<String> = sqlx::query_scalar("SELECT id FROM source_preparations
                    WHERE user_id = ? AND recording_id = ? AND source_id = ?
                        AND (phase IN ('queued','submitting','preparing','blocked','submission_unknown')
                            OR submission_state != 'unsubmitted') LIMIT 1")
                    .bind(actor.id()).bind(recording).bind(source).fetch_optional(&mut *tx).await?;
                let (preparation, outcome) = match retained {
                    Some(preparation) => (preparation, "coalesced"),
                    None => {
                        let target =
                            authority::target(&mut tx, actor.id(), recording, source).await?;
                        let connection = self
                            .plugins
                            .preparation_connection_on(&mut tx, actor.id(), &target.installation_id)
                            .await?;
                        if target.ready {
                            return Err(Error::Conflict("This source is already available".into()));
                        }
                        unresolved_origin(&mut tx, actor.id(), &target, &connection).await?;
                        pending_capacity(&mut tx, actor.id()).await?;
                        let preparation = Uuid::new_v4().to_string();
                        insert(
                            &mut tx,
                            &preparation,
                            actor.id(),
                            recording,
                            source,
                            &target,
                            &connection,
                        )
                        .await?;
                        authority::check_on(&mut tx, &preparation).await?;
                        (preparation, "queued")
                    }
                };
                sqlx::query("INSERT INTO source_preparation_receipts(user_id,request_id,request_digest,preparation_id) VALUES (?,?,?,?)")
                    .bind(actor.id()).bind(request.request_id.to_string()).bind(&digest).bind(&preparation)
                    .execute(&mut *tx).await?;
                (preparation, outcome)
            }
        };
        let receipt = SourcePreparationReceipt {
            request_id: request.request_id,
            preparation_id: identity(&preparation)?,
            recording_id: recording.into(),
            source_id: source.into(),
        };
        tx.commit().await?;
        tracing::info!(
            event = "source_preparation_requested",
            preparation_id = preparation,
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source preparation request recorded"
        );
        Ok(receipt)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn source_preparation(
        &self,
        actor: &Actor,
        recording: &str,
        source: &str,
    ) -> Result<SourcePreparationStatus> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let preparation = latest(&mut tx, actor.id(), recording, source)
            .await?
            .ok_or(Error::NotFound)?;
        authority::cancel_stale_on(&mut tx, &preparation).await?;
        let row: StatusRow = sqlx::query_as("SELECT id,recording_id,source_id,phase,progress_basis_points,
            eta_seconds,reason,next_check_at,created_at,updated_at FROM source_preparations WHERE id = ? AND user_id = ?")
            .bind(preparation).bind(actor.id()).fetch_one(&mut *tx).await?;
        let status = row.into_status()?;
        tx.commit().await?;
        Ok(status)
    }
}

async fn latest(
    connection: &mut SqliteConnection,
    account: &str,
    recording: &str,
    source: &str,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar("SELECT id FROM source_preparations
        WHERE user_id = ? AND recording_id = ? AND source_id = ? ORDER BY created_at DESC,rowid DESC LIMIT 1")
        .bind(account).bind(recording).bind(source).fetch_optional(connection).await?)
}

async fn replay(
    connection: &mut SqliteConnection,
    account: &str,
    request: Uuid,
    digest: &str,
) -> Result<Option<String>> {
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,preparation_id
        FROM source_preparation_receipts WHERE user_id = ? AND request_id = ?",
    )
    .bind(account)
    .bind(request.to_string())
    .fetch_optional(connection)
    .await?;
    previous
        .map(|(saved, preparation)| {
            if saved != digest {
                Err(Error::Conflict(
                    "A preparation request ID was reused for a different source".into(),
                ))
            } else {
                Ok(preparation)
            }
        })
        .transpose()
}

fn fingerprint(recording: &str, source: &str) -> Result<String> {
    let bytes = serde_json::to_vec(&("hito.source-preparation.request.v1", recording, source))
        .map_err(|_| Error::Internal("Source preparation request could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

async fn receipt_capacity(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let (account_count, total): (i64, i64) = sqlx::query_as(
        "SELECT coalesce(sum(user_id = ?),0),count(*) FROM source_preparation_receipts",
    )
    .bind(account)
    .fetch_one(connection)
    .await?;
    if account_count >= MAX_ACCOUNT_RECEIPTS || total >= MAX_RECEIPTS {
        return Err(Error::Conflict(
            "The retained source preparation request limit has been reached".into(),
        ));
    }
    Ok(())
}

async fn pending_capacity(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let (account_count, total): (i64, i64) = sqlx::query_as("SELECT coalesce(sum(user_id = ?),0),count(*)
        FROM source_preparations WHERE phase IN ('queued','submitting','preparing','blocked','submission_unknown')")
        .bind(account).fetch_one(connection).await?;
    if account_count >= MAX_ACCOUNT_PENDING || total >= MAX_PENDING {
        return Err(Error::Conflict(
            "The pending source preparation limit has been reached".into(),
        ));
    }
    Ok(())
}

async fn unresolved_origin(
    connection: &mut SqliteConnection,
    account: &str,
    target: &authority::Target,
    provider: &PreparationConnection,
) -> Result<()> {
    let unresolved: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM source_preparations
        WHERE user_id = ? AND service_id = ? AND provider = ? AND provider_recording_id = ?
            AND remote_source_id = ? AND submission_state IN ('submitting','unknown'))",
    )
    .bind(account)
    .bind(&provider.service_id)
    .bind(&provider.provider)
    .bind(&target.provider_recording_id)
    .bind(&target.remote_source_id)
    .fetch_one(connection)
    .await?;
    if unresolved {
        return Err(Error::Conflict(
            "A previous preparation for this provider source needs reconciliation".into(),
        ));
    }
    Ok(())
}

async fn insert(
    connection: &mut SqliteConnection,
    preparation: &str,
    account: &str,
    recording: &str,
    source: &str,
    target: &authority::Target,
    provider: &PreparationConnection,
) -> Result<()> {
    sqlx::query("INSERT INTO source_preparations(id,user_id,recording_id,source_id,library_id,access_grant,
        binding_id,binding_revision,provider_recording_id,remote_source_id,installation_id,installation_revision,
        service_id,service_revision,provider,credential,audio_generation) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(preparation).bind(account).bind(recording).bind(source).bind(&target.library_id).bind(&target.access_grant)
        .bind(&target.binding_id).bind(target.binding_revision).bind(&target.provider_recording_id).bind(&target.remote_source_id)
        .bind(&provider.installation_id).bind(provider.installation_revision).bind(&provider.service_id).bind(provider.service_revision)
        .bind(&provider.provider).bind(&provider.credential).bind(provider.audio_generation)
        .execute(connection).await?;
    Ok(())
}
