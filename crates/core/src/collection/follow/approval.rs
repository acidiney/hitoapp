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

fn compatible(scope: &str) -> String {
    format!(
        "SELECT f.playlist_id FROM playlist_follows f
        JOIN playlists p ON p.id = f.playlist_id AND p.user_id = f.user_id AND p.deleted = 0 AND p.kind = 'followed'
        JOIN users account ON account.id = f.user_id
        JOIN plugin_installations i ON i.id = f.installation_id AND i.user_id = f.user_id AND i.active = 1
            AND i.service_id = f.service_id AND json_extract(i.manifest,'$.id') = f.provider
        JOIN plugin_services s ON s.id = f.service_id AND s.active = 1
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

    pub(super) async fn resume_compatible_follows(&self, playlist: Option<&str>) -> Result<u64> {
        let started = Instant::now();
        let scope = if playlist.is_some() {
            "AND f.playlist_id = ?"
        } else {
            ""
        };
        let candidates = compatible(scope);
        let probe_sql = format!("SELECT EXISTS({candidates})");
        let mut probe = sqlx::query_scalar(&probe_sql);
        if let Some(playlist) = playlist {
            probe = probe.bind(playlist);
        }
        if !probe.fetch_one(&self.store.pool).await? {
            self.report_malformed_approvals().await?;
            return Ok(0);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let update_sql = format!(
            "UPDATE playlist_follows SET
                installation_revision = (SELECT i.revision FROM plugin_installations i WHERE i.id = playlist_follows.installation_id),
                service_revision = (SELECT s.revision FROM plugin_services s WHERE s.id = playlist_follows.service_id),
                approved_capabilities = (SELECT json_extract(i.manifest,'$.capabilities') FROM plugin_installations i WHERE i.id = playlist_follows.installation_id),
                approved_credential = (SELECT {CREDENTIAL_MARKER} FROM plugin_installations i WHERE i.id = playlist_follows.installation_id),
                generation = generation + 1,error = NULL,next_refresh_at = unixepoch()
             WHERE playlist_id IN ({candidates}) RETURNING playlist_id,generation"
        );
        let mut update = sqlx::query_as::<_, (String, i64)>(&update_sql);
        if let Some(playlist) = playlist {
            update = update.bind(playlist);
        }
        let resumed = update.fetch_all(&mut *tx).await?;
        let mut cancelled = 0;
        for (playlist, generation) in &resumed {
            cancelled += sqlx::query("UPDATE playlist_follow_jobs SET status = 'cancelled',error = 'approval_changed',updated_at = unixepoch()
                WHERE playlist_id = ? AND generation = ? AND status = 'queued'")
                .bind(playlist).bind(generation - 1).execute(&mut *tx).await?.rows_affected();
            history::prune(&mut tx, playlist).await?;
        }
        tx.commit().await?;
        if !resumed.is_empty() {
            tracing::info!(
                resumed_follows = resumed.len(),
                cancelled_jobs = cancelled,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Compatible playlist follows resumed after a connection revision change"
            );
        }
        Ok(resumed.len() as u64)
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
        "SELECT f.provider,f.service_id,f.approved_capabilities,f.approved_credential,
            i.manifest,i.revision AS installation_revision,i.active AS installation_active,i.service_id AS installation_service,
            ({CREDENTIAL_MARKER}) AS marker,s.active AS service_active,s.revision AS service_revision
         FROM playlist_follows f
         LEFT JOIN plugin_installations i ON i.id = f.installation_id
         LEFT JOIN plugin_services s ON s.id = f.service_id
         WHERE f.playlist_id = ?"
    ))
    .bind(playlist)
    .fetch_one(connection)
    .await?;
    let provider: String = row.try_get("provider")?;
    let service: String = row.try_get("service_id")?;
    let approved: Option<String> = row.try_get("approved_capabilities")?;
    let approved_credential: Option<String> = row.try_get("approved_credential")?;
    let manifest: Option<String> = row.try_get("manifest")?;
    let manifest = manifest.and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let name = manifest
        .as_ref()
        .and_then(|value| value["name"].as_str())
        .unwrap_or("Original")
        .to_owned();
    let identical = manifest
        .as_ref()
        .is_some_and(|value| value["id"].as_str() == Some(provider.as_str()))
        && row.try_get::<Option<i64>, _>("installation_active")? == Some(1)
        && row
            .try_get::<Option<String>, _>("installation_service")?
            .as_deref()
            == Some(service.as_str())
        && row.try_get::<Option<i64>, _>("service_active")? == Some(1);
    if !identical {
        return Ok(PlaylistFollowReview {
            connection_name: name,
            installation_revision: 0,
            service_revision: 0,
            resumable: false,
            changes: vec![PlaylistFollowChange {
                kind: PlaylistFollowChangeKind::Removed,
                before: vec!["Connected".into()],
                after: vec!["Removed or replaced".into()],
            }],
        });
    }
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
    })
}
