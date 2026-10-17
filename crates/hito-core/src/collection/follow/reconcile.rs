use crate::Result;
use hito_plugin_sdk::{ProviderPlaylistEntry, ProviderPlaylistItem};
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct PreviousEntry {
    entry_id: String,
    occurrence_id: Option<String>,
    provider_recording_id: Option<String>,
    kind: String,
}

pub(super) async fn identities(
    connection: &mut SqliteConnection,
    playlist: &str,
    entries: &[ProviderPlaylistEntry],
) -> Result<Vec<Uuid>> {
    let previous: Vec<PreviousEntry> = sqlx::query_as("SELECT provenance.entry_id,provenance.occurrence_id,provenance.provider_recording_id,provenance.kind
        FROM playlist_follow_entries provenance JOIN playlist_entries e ON e.playlist_id = provenance.playlist_id AND e.id = provenance.entry_id
        WHERE provenance.playlist_id = ? AND e.deleted = 0 ORDER BY e.position,e.id")
        .bind(playlist).fetch_all(connection).await?;
    let mut occurrences = HashMap::new();
    let mut recordings: HashMap<(String, String), VecDeque<Uuid>> = HashMap::new();
    for entry in previous {
        let id = super::receipts::identity(&entry.entry_id)?;
        if let Some(occurrence) = entry.occurrence_id {
            occurrences.insert(occurrence, (id, entry.kind, entry.provider_recording_id));
        } else if let Some(recording) = entry.provider_recording_id {
            recordings
                .entry((entry.kind, recording))
                .or_default()
                .push_back(id);
        }
    }
    let mut used = HashSet::new();
    Ok(entries
        .iter()
        .map(|entry| {
            let (kind, recording) = match &entry.item {
                ProviderPlaylistItem::Recording { recording } => {
                    ("recording", Some(recording.id.as_str()))
                }
                ProviderPlaylistItem::Unavailable {
                    provider_recording_id,
                    ..
                } => ("unavailable", provider_recording_id.as_deref()),
                ProviderPlaylistItem::Unknown {} => ("unknown", None),
            };
            let retained = if let Some(occurrence) = &entry.occurrence_id {
                occurrences
                    .get(occurrence)
                    .filter(|(_, old_kind, old_recording)| {
                        old_recording.as_deref() == recording
                            && (old_kind == kind || recording.is_some())
                    })
                    .map(|(id, _, _)| *id)
            } else {
                recording.and_then(|recording| {
                    recordings
                        .get_mut(&(kind.into(), recording.into()))
                        .and_then(VecDeque::pop_front)
                })
            };
            retained
                .filter(|id| used.insert(*id))
                .unwrap_or_else(Uuid::new_v4)
        })
        .collect())
}
