use super::Collection;
use super::history;
use super::types::{PlaylistFollowChange, PlaylistFollowChangeKind, PlaylistFollowReview};
use crate::Result;
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static REPORTED: AtomicBool = AtomicBool::new(false);

const CREDENTIAL_MARKER: &str =
    "CASE WHEN i.credential IS NULL THEN NULL ELSE hex(substr(i.credential,34,24)) END";

pub(super) const CONNECTION: &str = "CASE WHEN EXISTS(SELECT 1 FROM plugin_installations bound WHERE bound.id = f.installation_id AND bound.active = 1)
    THEN f.installation_id
    ELSE (SELECT successor.id FROM plugin_installations successor
        JOIN plugin_services offered ON offered.id = successor.service_id AND offered.active = 1
        WHERE successor.user_id = f.user_id AND successor.active = 1 AND json_extract(successor.manifest,'$.id') = f.provider
        ORDER BY successor.rowid DESC LIMIT 1) END";

pub(super) enum Scope<'a> {
    All,
    Playlist(&'a str),
    Account(&'a str),
}

impl Scope<'_> {
    fn filter(&self) -> &'static str {
        match self {
            Self::All => "",
            Self::Playlist(_) => "AND f.playlist_id = ?",
            Self::Account(_) => "AND f.user_id = ?",
        }
    }

    fn value(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Playlist(value) | Self::Account(value) => Some(value),
        }
    }
}

fn compatible(scope: &str) -> String {
    format!(
        "SELECT f.playlist_id,f.generation,i.id AS connection FROM playlist_follows f
        JOIN playlists p ON p.id = f.playlist_id AND p.user_id = f.user_id AND p.deleted = 0 AND p.kind = 'followed'
        JOIN users account ON account.id = f.user_id
        JOIN plugin_installations i ON i.id = ({CONNECTION}) AND i.user_id = f.user_id AND i.active = 1
            AND json_extract(i.manifest,'$.id') = f.provider
        JOIN plugin_services s ON s.id = i.service_id AND s.active = 1
        WHERE f.active = 1 {scope}
            AND NOT EXISTS(SELECT 1 FROM current_playlist_follow_authority a WHERE a.playlist_id = f.playlist_id)
            AND EXISTS(SELECT 1 FROM json_each(i.manifest,'$.capabilities') WHERE value = 'playlists')
            AND f.approved_capabilities IS NOT NULL AND json_valid(f.approved_capabilities)
            AND CASE WHEN json_valid(f.approved_capabilities) THEN
                NOT EXISTS(SELECT 1 FROM json_each(i.manifest,'$.capabilities') c
                    WHERE c.value NOT IN (SELECT value FROM json_each(f.approved_capabilities)))
                AND f.approved_credential IS ({CREDENTIAL_MARKER})
                ELSE 0 END"
    )
}

pub(super) async fn record_baseline(
    connection: &mut SqliteConnection,
    playlist: &str,
) -> Result<()> {
    sqlx::query(&format!(
        "UPDATE playlist_follows SET
            approved_capabilities = (SELECT json_extract(i.manifest,'$.capabilities') FROM plugin_installations i WHERE i.id = playlist_follows.installation_id),
            approved_credential = (SELECT {CREDENTIAL_MARKER} FROM plugin_installations i WHERE i.id = playlist_follows.installation_id)
         WHERE playlist_id = ?"
    ))
    .bind(playlist)
    .execute(connection)
    .await?;
    Ok(())
}

async fn rebind_compatible(
    connection: &mut SqliteConnection,
    scope: &Scope<'_>,
) -> Result<(u64, u64)> {
    let candidates = compatible(scope.filter());
    let mut query = sqlx::query_as::<_, (String, i64, String)>(&candidates);
    if let Some(value) = scope.value() {
        query = query.bind(value);
    }
    let found = query.fetch_all(&mut *connection).await?;
    let update = format!(
        "UPDATE playlist_follows SET installation_id = i.id,service_id = i.service_id,
            installation_revision = i.revision,service_revision = s.revision,
            approved_capabilities = json_extract(i.manifest,'$.capabilities'),approved_credential = {CREDENTIAL_MARKER},
            generation = playlist_follows.generation + 1,error = NULL,next_refresh_at = unixepoch()
         FROM plugin_installations i JOIN plugin_services s ON s.id = i.service_id
         WHERE playlist_follows.playlist_id = ? AND playlist_follows.generation = ? AND i.id = ?"
    );
    let mut resumed = 0;
    let mut cancelled = 0;
    for (playlist, generation, installation) in &found {
        let changed = sqlx::query(&update)
            .bind(playlist)
            .bind(generation)
            .bind(installation)
            .execute(&mut *connection)
            .await?
            .rows_affected();
        if changed != 1 {
            continue;
        }
        resumed += 1;
        cancelled += sqlx::query("UPDATE playlist_follow_jobs SET status = 'cancelled',error = 'approval_changed',updated_at = unixepoch()
            WHERE playlist_id = ? AND generation = ? AND status = 'queued'")
            .bind(playlist).bind(generation).execute(&mut *connection).await?.rows_affected();
        history::prune(connection, playlist).await?;
    }
    Ok((resumed, cancelled))
}

pub(crate) async fn carry_over_follows(
    connection: &mut SqliteConnection,
    account: &str,
    provider: &str,
) -> Result<()> {
    let started = Instant::now();
    let (carried, cancelled) = rebind_compatible(connection, &Scope::Account(account)).await?;
    let awaiting_review = sqlx::query(
        "UPDATE playlist_follows SET generation = generation + 1
         WHERE user_id = ? AND active = 1 AND provider = ?
            AND EXISTS(SELECT 1 FROM plugin_installations bound WHERE bound.id = playlist_follows.installation_id AND bound.active = 0)",
    )
    .bind(account)
    .bind(provider)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if carried > 0 || awaiting_review > 0 {
        tracing::info!(
            carried_follows = carried,
            follows_awaiting_review = awaiting_review,
            cancelled_jobs = cancelled,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Playlist follows carried over to a new plugin connection"
        );
    }
    Ok(())
}

impl Collection {
    async fn report_malformed_approvals(&self) -> Result<()> {
        if REPORTED.load(Ordering::Relaxed) {
            return Ok(());
        }
        let malformed: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_follows WHERE approved_capabilities IS NOT NULL AND NOT json_valid(approved_capabilities)")
            .fetch_one(&self.store.pool)
            .await?;
        if malformed > 0 && !REPORTED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                malformed_follows = malformed,
                "Playlist follows with an unreadable approval record stay paused"
            );
        }
        Ok(())
    }

    pub(super) async fn resume_compatible_follows(&self, scope: Scope<'_>) -> Result<u64> {
        let started = Instant::now();
        let probe_sql = format!("SELECT EXISTS({})", compatible(scope.filter()));
        let mut probe = sqlx::query_scalar(&probe_sql);
        if let Some(value) = scope.value() {
            probe = probe.bind(value);
        }
        if !probe.fetch_one(&self.store.pool).await? {
            self.report_malformed_approvals().await?;
            return Ok(0);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (resumed, cancelled) = rebind_compatible(&mut tx, &scope).await?;
        tx.commit().await?;
        if resumed > 0 {
            tracing::info!(
                resumed_follows = resumed,
                cancelled_jobs = cancelled,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Compatible playlist follows resumed after a connection revision change"
            );
        }
        Ok(resumed)
    }
}

fn names(value: Option<&str>) -> Vec<String> {
    value
        .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
        .unwrap_or_default()
}

fn token_state(marker: &Option<String>) -> String {
    if marker.is_some() {
        "Saved token".into()
    } else {
        "No token".into()
    }
}

pub(super) async fn review(
    connection: &mut SqliteConnection,
    playlist: &str,
) -> Result<PlaylistFollowReview> {
    let row = sqlx::query(&format!(
        "SELECT f.provider,f.approved_capabilities,f.approved_credential,
            bound.manifest AS bound_manifest,i.manifest,i.revision AS installation_revision,
            i.active AS installation_active,i.user_id = f.user_id AS owned,
            ({CREDENTIAL_MARKER}) AS marker,s.active AS service_active,s.revision AS service_revision,
            (SELECT offered.id FROM plugin_services offered WHERE offered.id = f.service_id AND offered.active = 1
                AND NOT EXISTS(SELECT 1 FROM plugin_installations mine WHERE mine.user_id = f.user_id
                    AND mine.service_id = offered.id AND mine.active = 1)) AS reconnect_service_id
         FROM playlist_follows f
         LEFT JOIN plugin_installations bound ON bound.id = f.installation_id
         LEFT JOIN plugin_installations i ON i.id = ({CONNECTION})
         LEFT JOIN plugin_services s ON s.id = i.service_id
         WHERE f.playlist_id = ?"
    ))
    .bind(playlist)
    .fetch_one(connection)
    .await?;
    let provider: String = row.try_get("provider")?;
    let approved: Option<String> = row.try_get("approved_capabilities")?;
    let approved_credential: Option<String> = row.try_get("approved_credential")?;
    let parse = |text: Option<String>| {
        text.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    };
    let manifest = parse(row.try_get("manifest")?);
    let bound = parse(row.try_get("bound_manifest")?);
    let identical = manifest
        .as_ref()
        .is_some_and(|value| value["id"].as_str() == Some(provider.as_str()))
        && row.try_get::<Option<i64>, _>("installation_active")? == Some(1)
        && row.try_get::<Option<bool>, _>("owned")? == Some(true)
        && row.try_get::<Option<i64>, _>("service_active")? == Some(1);
    if !identical {
        return Ok(PlaylistFollowReview {
            connection_name: connection_name(bound.as_ref()),
            installation_revision: 0,
            service_revision: 0,
            resumable: false,
            changes: vec![PlaylistFollowChange {
                kind: PlaylistFollowChangeKind::Removed,
                before: vec!["Connected".into()],
                after: vec!["Removed or replaced".into()],
            }],
            reconnect_service_id: row.try_get("reconnect_service_id")?,
        });
    }
    let name = connection_name(manifest.as_ref());
    let current: Vec<String> = manifest
        .as_ref()
        .and_then(|value| value["capabilities"].as_array())
        .map(|list| {
            list.iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut changes = Vec::new();
    let before = names(approved.as_deref());
    let current_set: BTreeSet<&String> = current.iter().collect();
    if approved.is_none() || before.iter().collect::<BTreeSet<_>>() != current_set {
        changes.push(PlaylistFollowChange {
            kind: PlaylistFollowChangeKind::Capabilities,
            before,
            after: current.clone(),
        });
    }
    let marker: Option<String> = row.try_get("marker")?;
    if approved.is_some() && approved_credential != marker {
        let replaced = approved_credential.is_some() && marker.is_some();
        changes.push(PlaylistFollowChange {
            kind: PlaylistFollowChangeKind::Credential,
            before: vec![if replaced {
                "Previous token".into()
            } else {
                token_state(&approved_credential)
            }],
            after: vec![if replaced {
                "New token".into()
            } else {
                token_state(&marker)
            }],
        });
    }
    let resumable = current.iter().any(|value| value == "playlists");
    if !resumable && changes.is_empty() {
        changes.push(PlaylistFollowChange {
            kind: PlaylistFollowChangeKind::Capabilities,
            before: vec!["playlists".into()],
            after: current,
        });
    }
    Ok(PlaylistFollowReview {
        connection_name: name,
        installation_revision: row.try_get("installation_revision")?,
        service_revision: row.try_get("service_revision")?,
        resumable,
        changes,
        reconnect_service_id: None,
    })
}

fn connection_name(manifest: Option<&serde_json::Value>) -> String {
    manifest
        .and_then(|value| value["name"].as_str())
        .unwrap_or("Original")
        .to_owned()
}
