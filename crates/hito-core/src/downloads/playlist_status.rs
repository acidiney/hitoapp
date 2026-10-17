use super::{
    Downloads, PlaylistDownloadEntries, PlaylistDownloadEntry, PlaylistDownloadEntryState,
    PlaylistDownloadQuery, PlaylistDownloadStatus, health, roots::identity,
};
use crate::{Actor, Error, Result, library_storage::ManagedFileLocation};
use sqlx::{QueryBuilder, Row, sqlite::SqliteRow};

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn playlist_status(
        &self,
        actor: &Actor,
        playlist: &str,
        query: PlaylistDownloadQuery,
    ) -> Result<PlaylistDownloadStatus> {
        let playlist = identity(playlist)?;
        for _ in 0..3 {
            let (status, copies) = self.read_playlist_status(actor, &playlist, &query).await?;
            let mut changed = false;
            for copy in copies {
                if !self.files.available(&copy).await? {
                    health::retire_unusable_copy(&self.store, &copy).await?;
                    changed = true;
                }
            }
            if changed {
                continue;
            }
            let (current, _) = self.read_playlist_status(actor, &playlist, &query).await?;
            if current == status {
                return Ok(current);
            }
        }
        Err(Error::Conflict(
            "Playlist download state changed. Try again.".into(),
        ))
    }

    async fn read_playlist_status(
        &self,
        actor: &Actor,
        playlist: &str,
        page: &PlaylistDownloadQuery,
    ) -> Result<(PlaylistDownloadStatus, Vec<ManagedFileLocation>)> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let (retention, revision, reconciled): (String, i64, i64) = sqlx::query_as("SELECT root.id,changes.revision,work.reconciled_revision
            FROM download_retentions root JOIN playlists p ON p.id=root.playlist_id AND p.user_id=root.account_id AND p.deleted=0
            JOIN download_playlist_changes changes ON changes.playlist_id=p.id
            JOIN download_playlist_work work ON work.retention_id=root.id AND work.host_device=root.host_device
            WHERE root.account_id=? AND root.host_device=? AND root.playlist_id=? AND root.kind='playlist' AND root.state='active'")
            .bind(actor.id()).bind(self.host.as_str()).bind(playlist).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let (after, limit) = page.selection(&retention, revision)?;
        let mut query = QueryBuilder::new("SELECT e.id AS entry_id,e.recording_id,access.id AS access_grant_id,m.seen_revision,
            i.state,i.error,f.source_id,f.id AS file_id,f.attempt_id,f.generation AS file_generation,f.checksum,f.byte_length,f.mime,
            EXISTS(SELECT 1 FROM download_playlist_retry_waiting pending
                JOIN download_playlist_retry_work command ON command.retention_id=pending.retention_id
                    AND command.host_device=pending.host_device AND command.command_generation=pending.command_generation
                JOIN download_terminal_failures failure ON failure.intent_id=pending.failed_intent_id
                WHERE pending.retention_id=m.retention_id AND pending.recording_id=e.recording_id
                    AND pending.host_device=i.host_device AND pending.failed_intent_id=i.id
                    AND i.state='failed' AND failure.sequence<=command.failure_cutoff) AS retry_waiting,
            (EXISTS(SELECT 1 FROM sources s JOIN provider_sources ps ON ps.source_id=s.id AND ps.recording_id=s.recording_id
                JOIN provider_items item ON item.id=ps.binding_id AND item.retired=0
                JOIN provider_source_state source_state ON source_state.binding_id=item.id AND source_state.generation=ps.generation
                JOIN plugin_installations installation ON installation.id=ps.installation_id AND installation.active=1
                JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
                JOIN source_choice_bindings permitted ON permitted.binding_id=item.id
                    AND permitted.recording_id=s.recording_id AND permitted.user_id=installation.user_id
                WHERE s.recording_id=e.recording_id AND s.ready=1 AND installation.user_id=");
        query.push_bind(actor.id().to_owned()).push(" AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='sources'))
            OR EXISTS(SELECT 1 FROM current_podcast_sources enclosure WHERE enclosure.recording_id=e.recording_id AND enclosure.user_id=")
            .push_bind(actor.id().to_owned()).push(" AND NOT EXISTS(SELECT 1 FROM recording_source_choices choice
                WHERE choice.user_id=enclosure.user_id AND choice.recording_id=enclosure.recording_id AND choice.binding_id IS NOT NULL))) AS source_ready
            FROM playlist_entries e LEFT JOIN recordings recording ON recording.id=e.recording_id
            LEFT JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=")
            .push_bind(actor.id().to_owned()).push(" LEFT JOIN download_playlist_members m ON m.entry_id=e.id AND m.recording_id IS e.recording_id AND m.retention_id=")
            .push_bind(&retention).push(" LEFT JOIN download_intents i ON i.recording_id=e.recording_id AND i.account_id=")
            .push_bind(actor.id().to_owned()).push(" AND i.host_device=").push_bind(self.host.as_str().to_owned())
            .push(" AND i.state!='removed' AND i.access_grant_id=access.id
            AND EXISTS(SELECT 1 FROM retained_download_intents retained WHERE retained.id=i.id)
            LEFT JOIN download_files f ON f.id=i.file_id AND f.account_id=i.account_id AND f.recording_id=i.recording_id
                AND f.host_device=i.host_device AND f.state='ready' WHERE e.playlist_id=")
            .push_bind(playlist).push(" AND e.deleted=0");
        if let Some(after) = after {
            query.push(" AND e.id>").push_bind(after);
        }
        query.push(" ORDER BY e.id LIMIT ").push_bind(limit + 1);
        let mut rows = query.build().fetch_all(&mut *tx).await?;
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let mut items = Vec::with_capacity(rows.len());
        let mut copies = Vec::new();
        for row in rows {
            let item = entry(&row, revision)?;
            if item.status == PlaylistDownloadEntryState::Ready {
                copies.push(health::ready_file(&row)?);
            }
            items.push(item);
        }
        let next_cursor = if more {
            items
                .last()
                .map(|last| format!("{retention}:{revision}:{}", last.entry_id))
        } else {
            None
        };
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(retention_id = %retention, revision, entry_count = items.len(), more, "Playlist download page read");
        Ok((
            PlaylistDownloadStatus {
                retention_id: retention,
                playlist_id: playlist.into(),
                host_device_id: self.host.as_str().into(),
                desired_revision: revision,
                reconciled_revision: reconciled,
                entries: PlaylistDownloadEntries { items, next_cursor },
            },
            copies,
        ))
    }
}

fn entry(row: &SqliteRow, revision: i64) -> Result<PlaylistDownloadEntry> {
    use PlaylistDownloadEntryState as State;
    let recording: Option<String> = row.try_get("recording_id")?;
    let authorized = row
        .try_get::<Option<String>, _>("access_grant_id")?
        .is_some();
    let projected = row.try_get::<Option<i64>, _>("seen_revision")? == Some(revision);
    let (status, error) = if recording.is_none() || !authorized {
        (State::Unavailable, Some("unavailable_item".into()))
    } else if !projected {
        (State::Pending, None)
    } else {
        match row.try_get::<Option<&str>, _>("state")? {
            Some("ready") if row.try_get::<Option<String>, _>("source_id")?.is_some() => {
                (State::Ready, None)
            }
            Some("queued") => (State::Queued, None),
            Some("running") => (State::Running, None),
            Some("verifying") => (State::Verifying, None),
            Some("failed") if row.try_get::<bool, _>("retry_waiting")? => {
                (State::WaitingForSource, None)
            }
            Some(_) => (State::Failed, row.try_get("error")?),
            None if row.try_get::<bool, _>("source_ready")? => (State::Pending, None),
            None => (State::WaitingForSource, None),
        }
    };
    let source_id = if status == State::Ready {
        row.try_get("source_id")?
    } else {
        None
    };
    Ok(PlaylistDownloadEntry {
        entry_id: row.try_get("entry_id")?,
        recording_id: if authorized { recording } else { None },
        status,
        source_id,
        error,
    })
}
