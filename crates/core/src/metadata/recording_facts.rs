use super::{AudioFormat, InspectedTags, LastPlayed, ReleaseFacts};
use crate::{Actor, Playlist, Result};
use sqlx::{Row, SqliteConnection};

const APPEARANCE_LIMIT: i64 = 50;

pub(super) struct LocalFacts {
    pub audio_format: Option<AudioFormat>,
    pub release: ReleaseFacts,
}

pub(super) async fn local_file(
    connection: &mut SqliteConnection,
    recording: &str,
) -> Result<LocalFacts> {
    let row = sqlx::query(
        "SELECT o.tags, f.size_bytes FROM metadata_observations o
         JOIN local_files f ON f.source_id = o.source_id
         WHERE o.recording_id = ? AND o.origin = 'file_inspection'
         ORDER BY o.observed_at DESC, o.rowid DESC LIMIT 1",
    )
    .bind(recording)
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Ok(LocalFacts {
            audio_format: None,
            release: ReleaseFacts {
                year: None,
                track_total: None,
                label: None,
            },
        });
    };
    let tags: Option<InspectedTags> = row
        .try_get::<Option<String>, _>("tags")?
        .and_then(|tags| serde_json::from_str(&tags).ok());
    let audio = tags.as_ref().and_then(|tags| tags.audio.as_ref());
    Ok(LocalFacts {
        audio_format: Some(AudioFormat {
            format: audio.map(|audio| audio.format.clone()),
            bit_depth: audio.and_then(|audio| audio.bit_depth),
            sample_rate_hz: audio.and_then(|audio| audio.sample_rate_hz),
            size_bytes: row.try_get("size_bytes")?,
        }),
        release: tags.as_ref().map(release_facts).unwrap_or(ReleaseFacts {
            year: None,
            track_total: None,
            label: None,
        }),
    })
}

fn release_facts(tags: &InspectedTags) -> ReleaseFacts {
    let from_track = tags
        .track
        .as_ref()
        .and_then(|track| track.value.split_once('/'))
        .and_then(|(_, total)| positive(total));
    ReleaseFacts {
        year: tags.date.as_ref().and_then(|date| year(&date.value)),
        track_total: from_track.or_else(|| {
            tags.track_total
                .as_ref()
                .and_then(|total| positive(&total.value))
        }),
        label: tags
            .label
            .as_ref()
            .map(|label| label.value.trim().to_owned())
            .filter(|label| !label.is_empty() && label.len() <= 256),
    }
}

fn positive(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok().filter(|value| *value > 0)
}

fn year(text: &str) -> Option<i32> {
    let digits: String = text.trim().chars().take(4).collect();
    digits
        .parse::<i32>()
        .ok()
        .filter(|year| digits.len() == 4 && (1000..=2999).contains(year))
}

pub(super) async fn last_played(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Option<LastPlayed>> {
    Ok(sqlx::query(
        "SELECT updated_at, position_ms FROM listening_progress
         WHERE user_id = ? AND recording_id = ?",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(connection)
    .await?
    .map(|row| LastPlayed {
        played_at: row.get("updated_at"),
        position_ms: row.get("position_ms"),
    }))
}

pub(super) async fn playlists_containing(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Vec<Playlist>> {
    Ok(sqlx::query_as(
        "SELECT p.id, p.name, p.kind FROM playlists p
         WHERE p.user_id = ? AND p.deleted = 0
           AND EXISTS (SELECT 1 FROM playlist_entries e
                       WHERE e.playlist_id = p.id AND e.recording_id = ? AND e.deleted = 0)
         ORDER BY p.name, p.id LIMIT ?",
    )
    .bind(actor.id())
    .bind(recording)
    .bind(APPEARANCE_LIMIT)
    .fetch_all(connection)
    .await?)
}
