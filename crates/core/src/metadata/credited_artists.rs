use super::CreditedArtistView;
use crate::{Actor, Result};
use hito_plugin_sdk::{CreditRole, CreditedArtist, ProviderRecording};
use sqlx::SqliteConnection;

fn role_name(role: CreditRole) -> &'static str {
    match role {
        CreditRole::Main => "main",
        CreditRole::Featured => "featured",
    }
}

fn role_of(name: &str) -> CreditRole {
    if name == "featured" {
        CreditRole::Featured
    } else {
        CreditRole::Main
    }
}

/// The credit a structured list states, as its main artists in the provider's order. Featured
/// artists are not part of it.
const MAX_IDENTIFYING_PAGES: usize = 8;
const MAX_LABEL_BYTES: usize = 4352;
const MAX_KEY_BYTES: usize = 8192;

pub(crate) fn credit_label(artists: &[CreditedArtist]) -> Option<String> {
    let mains: Vec<&str> = artists
        .iter()
        .filter(|artist| artist.role == CreditRole::Main)
        .map(|artist| artist.name.trim())
        .collect();
    (!mains.is_empty()).then(|| mains.join(" & "))
}

/// Identifies the same set of main artists in any order and casing.
pub(crate) fn credit_key(artists: &[CreditedArtist]) -> Option<String> {
    let mut mains: Vec<String> = artists
        .iter()
        .filter(|artist| artist.role == CreditRole::Main)
        .map(|artist| artist.name.trim().to_lowercase())
        .collect();
    mains.sort();
    (!mains.is_empty()).then(|| mains.join("\u{1f}"))
}

/// The artist pages that can identify the credit: those of its main artists, and only when every
/// main artist has one. A recording without a list keeps the pages the provider listed.
pub(crate) fn identifying_pages(details: &ProviderRecording) -> Vec<String> {
    if details.artists.is_empty() {
        return details.artist_urls.clone();
    }
    details
        .artists
        .iter()
        .filter(|artist| artist.role == CreditRole::Main)
        .map(|artist| artist.page_url.clone())
        .collect::<Option<Vec<String>>>()
        .filter(|pages| pages.len() <= MAX_IDENTIFYING_PAGES)
        .unwrap_or_default()
}

pub(crate) async fn retain(
    connection: &mut SqliteConnection,
    observation: &str,
    recording: &str,
    artists: &[CreditedArtist],
) -> Result<()> {
    let (Some(label), Some(key)) = (credit_label(artists), credit_key(artists)) else {
        return Ok(());
    };
    if label.len() > MAX_LABEL_BYTES || key.len() > MAX_KEY_BYTES {
        tracing::warn!(
            count = artists.len(),
            "Credited artists exceed the retention bounds"
        );
        return Ok(());
    }
    let retained: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM recording_credit_observations WHERE observation_id=?)",
    )
    .bind(observation)
    .fetch_one(&mut *connection)
    .await?;
    if retained {
        return Ok(());
    }
    sqlx::query("INSERT INTO recording_credit_observations(observation_id,recording_id,credit_label,credit_key) VALUES(?,?,?,?)")
        .bind(observation)
        .bind(recording)
        .bind(label)
        .bind(key)
        .execute(&mut *connection)
        .await?;
    for (position, artist) in artists.iter().enumerate() {
        sqlx::query("INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role,join_phrase,provider_artist_id,page_url) VALUES(?,?,?,?,?,?,?,?)")
            .bind(observation)
            .bind(position as i64)
            .bind(recording)
            .bind(artist.name.trim())
            .bind(role_name(artist.role))
            .bind(&artist.join)
            .bind(&artist.provider_artist_id)
            .bind(&artist.page_url)
            .execute(&mut *connection)
            .await?;
    }
    tracing::debug!(count = artists.len(), "Credited artists retained");
    Ok(())
}

/// The credited artists of the recording's current observation, each linked to the account's
/// artist page of exactly that name when it has one. A personal artist text hides them.
pub(crate) async fn read(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Vec<CreditedArtistView>> {
    let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT c.name,c.role,c.join_phrase,
            (SELECT a.id FROM account_artists a
              WHERE a.user_id=?1 AND a.library_id=r.library_id AND a.name=c.name COLLATE BINARY
                AND (EXISTS (SELECT 1 FROM account_recording_credits rc WHERE rc.user_id=a.user_id AND rc.artist_id=a.id)
                  OR EXISTS (SELECT 1 FROM account_artist_releases ar WHERE ar.user_id=a.user_id AND ar.artist_id=a.id))
              ORDER BY a.id LIMIT 1)
         FROM recordings r
         JOIN recording_credited_artist_observations c ON c.observation_id=r.metadata_observation_id AND c.recording_id=r.id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=?1
         WHERE r.id=?2 AND p.override_artist IS NULL
         ORDER BY c.position",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, role, join, artist_id)| CreditedArtistView {
            name,
            role: role_of(&role),
            join,
            artist_id,
        })
        .collect())
}
