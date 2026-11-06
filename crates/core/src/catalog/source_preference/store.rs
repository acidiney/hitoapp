use super::{
    SourcePreferenceEntry, SourcePreferences, UpdateSourcePreferences, ranking::SourceRanking,
};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet};

const MAX_ENTRIES: usize = 32;

pub(super) async fn read_on(
    connection: &mut SqliteConnection,
    actor: &Actor,
) -> Result<SourcePreferences> {
    let stored = stored_on(connection, actor).await?;
    let eligible = eligible_providers(connection, actor).await?;
    Ok(effective(stored, &eligible))
}

pub(crate) async fn ranking_on(
    connection: &mut SqliteConnection,
    actor: &Actor,
) -> Result<SourceRanking> {
    Ok(SourceRanking::new(read_on(connection, actor).await?))
}

pub(crate) async fn provider_installations(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT ps.source_id,item.installation_id FROM provider_items item INDEXED BY provider_items_recording
        CROSS JOIN provider_sources ps INDEXED BY provider_sources_generation ON ps.binding_id=item.id
        JOIN plugin_installations installation ON installation.id=item.installation_id AND installation.user_id=?
        WHERE item.recording_id=? AND item.retired=0",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_all(connection)
    .await?;
    Ok(rows.into_iter().collect())
}

pub(super) async fn write_on(
    connection: &mut SqliteConnection,
    actor: &Actor,
    request: UpdateSourcePreferences,
) -> Result<SourcePreferences> {
    let current = stored_on(connection, actor).await?;
    if current.revision != request.revision {
        return Err(Error::Conflict(
            "Source preferences changed. Review them again.".into(),
        ));
    }
    let revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| Error::Conflict("Source preferences cannot be changed further".into()))?;
    let eligible = eligible_providers(connection, actor).await?;
    validate(&request.entries, &eligible)?;
    let entries = serde_json::to_string(&request.entries)
        .map_err(|_| Error::Internal("Source preferences could not be encoded".into()))?;
    sqlx::query(
        "INSERT INTO account_source_preferences(user_id,revision,fallback,entries) VALUES(?,?,?,?)
        ON CONFLICT(user_id) DO UPDATE SET revision=excluded.revision,fallback=excluded.fallback,entries=excluded.entries",
    )
    .bind(actor.id())
    .bind(revision)
    .bind(request.fallback)
    .bind(entries)
    .execute(&mut *connection)
    .await?;
    read_on(connection, actor).await
}

fn validate(entries: &[SourcePreferenceEntry], eligible: &[String]) -> Result<()> {
    if entries.len() > MAX_ENTRIES {
        return Err(Error::Invalid(
            "Rank at most 32 audio source choices".into(),
        ));
    }
    let mut seen = HashSet::new();
    for entry in entries {
        if !seen.insert(entry) {
            return Err(Error::Invalid("Rank each audio source once".into()));
        }
        if let SourcePreferenceEntry::Provider { installation_id } = entry
            && !eligible.contains(installation_id)
        {
            return Err(Error::Invalid(
                "Rank only connected providers that can supply audio".into(),
            ));
        }
    }
    Ok(())
}

struct Stored {
    revision: i64,
    fallback: bool,
    entries: Vec<SourcePreferenceEntry>,
}

async fn stored_on(connection: &mut SqliteConnection, actor: &Actor) -> Result<Stored> {
    let row: Option<(i64, bool, String)> = sqlx::query_as(
        "SELECT revision,fallback,entries FROM account_source_preferences WHERE user_id=?",
    )
    .bind(actor.id())
    .fetch_optional(connection)
    .await?;
    let Some((revision, fallback, entries)) = row else {
        return Ok(Stored {
            revision: 0,
            fallback: true,
            entries: Vec::new(),
        });
    };
    Ok(Stored {
        revision,
        fallback,
        entries: serde_json::from_str(&entries)
            .map_err(|_| Error::Internal("Stored source preferences are unreadable".into()))?,
    })
}

async fn eligible_providers(
    connection: &mut SqliteConnection,
    actor: &Actor,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT installation.id FROM plugin_installations installation
        JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
        WHERE installation.user_id=? AND installation.active=1
        AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='sources')
        ORDER BY installation.rowid",
    )
    .bind(actor.id())
    .fetch_all(connection)
    .await?)
}

fn effective(stored: Stored, eligible: &[String]) -> SourcePreferences {
    let kept: Vec<SourcePreferenceEntry> = stored
        .entries
        .into_iter()
        .filter(|entry| match entry {
            SourcePreferenceEntry::Provider { installation_id } => {
                eligible.contains(installation_id)
            }
            _ => true,
        })
        .collect();
    let mut entries = Vec::with_capacity(kept.len() + eligible.len() + 2);
    for default in [
        SourcePreferenceEntry::LocalFiles,
        SourcePreferenceEntry::SavedCopies,
    ] {
        if !kept.contains(&default) {
            entries.push(default);
        }
    }
    entries.extend(kept);
    for installation_id in eligible {
        let entry = SourcePreferenceEntry::Provider {
            installation_id: installation_id.clone(),
        };
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    SourcePreferences {
        revision: stored.revision,
        fallback: stored.fallback,
        entries,
    }
}
