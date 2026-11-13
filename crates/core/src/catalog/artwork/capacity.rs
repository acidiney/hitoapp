use crate::{Error, Result};
use sqlx::SqliteConnection;

pub(super) const RESERVATION_BYTES: i64 = 2 * 1024 * 1024;
const ACCOUNT_BYTES: i64 = 32 * 1024 * 1024;
const GLOBAL_BYTES: i64 = 256 * 1024 * 1024;
const ACCOUNT_ASSETS: i64 = 1024;
const GLOBAL_ASSETS: i64 = 8192;

pub(super) struct Usage {
    account_bytes: i64,
    global_bytes: i64,
    account_assets: i64,
    global_assets: i64,
    account_pending: i64,
    global_pending: i64,
}

pub(super) async fn usage(
    connection: &mut SqliteConnection,
    account: &str,
    publishing_fetch: Option<&str>,
) -> Result<Usage> {
    let (account_assets, account_bytes): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets WHERE user_id = ?",
    )
    .bind(account)
    .fetch_one(&mut *connection)
    .await?;
    let (global_assets, global_bytes): (i64, i64) =
        sqlx::query_as("SELECT count(*),coalesce(sum(length(bytes)),0) FROM artwork_assets")
            .fetch_one(&mut *connection)
            .await?;
    let (account_pending, account_reserved): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(reservation_bytes),0) FROM artwork_fetches
         WHERE user_id = ? AND status IN ('queued','running') AND (? IS NULL OR id != ?)",
    )
    .bind(account)
    .bind(publishing_fetch)
    .bind(publishing_fetch)
    .fetch_one(&mut *connection)
    .await?;
    let (global_pending, global_reserved): (i64, i64) = sqlx::query_as(
        "SELECT count(*),coalesce(sum(reservation_bytes),0) FROM artwork_fetches
         WHERE status IN ('queued','running') AND (? IS NULL OR id != ? OR user_id != ?)",
    )
    .bind(publishing_fetch)
    .bind(publishing_fetch)
    .bind(account)
    .fetch_one(connection)
    .await?;
    Ok(Usage {
        account_bytes: account_bytes + account_reserved,
        global_bytes: global_bytes + global_reserved,
        account_assets: account_assets + account_pending,
        global_assets: global_assets + global_pending,
        account_pending,
        global_pending,
    })
}

pub(super) async fn admit_job(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let usage = usage(connection, account, None).await?;
    if usage.account_pending >= 16 || usage.global_pending >= 128 {
        return Err(Error::ArtworkAdmissionBusy);
    }
    admit(usage)
}

pub(super) async fn admit_image(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    admit(usage(connection, account, None).await?)
}

fn admit(usage: Usage) -> Result<()> {
    if usage.account_bytes + RESERVATION_BYTES > ACCOUNT_BYTES
        || usage.global_bytes + RESERVATION_BYTES > GLOBAL_BYTES
        || usage.account_assets + 1 > ACCOUNT_ASSETS
        || usage.global_assets + 1 > GLOBAL_ASSETS
    {
        return Err(Error::ArtworkStorageFull);
    }
    Ok(())
}

pub(super) fn publication(before: Usage, after: Usage) -> Result<()> {
    if !within(before.account_bytes, after.account_bytes, ACCOUNT_BYTES)
        || !within(before.global_bytes, after.global_bytes, GLOBAL_BYTES)
        || !within(before.account_assets, after.account_assets, ACCOUNT_ASSETS)
        || !within(before.global_assets, after.global_assets, GLOBAL_ASSETS)
    {
        return Err(Error::ArtworkStorageFull);
    }
    Ok(())
}

fn within(before: i64, after: i64, limit: i64) -> bool {
    after <= limit || after <= before
}
