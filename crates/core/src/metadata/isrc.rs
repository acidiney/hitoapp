use crate::Result;
use sqlx::SqliteConnection;

#[derive(Clone, Copy)]
pub(crate) enum IsrcOrigin {
    FileTag,
    ProviderResponse,
}

impl IsrcOrigin {
    fn name(self) -> &'static str {
        match self {
            Self::FileTag => "file_tag",
            Self::ProviderResponse => "provider_response",
        }
    }
}

/// Retains the artist pages an observation states. Only an observation naming exactly one page can
/// identify an artist, but every stated page is kept as evidence.
pub(crate) async fn retain_artist_pages(
    connection: &mut SqliteConnection,
    observation: &str,
    recording: &str,
    pages: &[String],
) -> Result<()> {
    for page in pages.iter().filter(|page| page.starts_with("https://")) {
        sqlx::query("INSERT OR IGNORE INTO recording_artist_url_observations(observation_id,recording_id,url) VALUES(?,?,?)")
            .bind(observation)
            .bind(recording)
            .bind(page)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

/// Retains the canonical ISRC an observation states, if any. A malformed value is ignored.
pub(crate) async fn retain(
    connection: &mut SqliteConnection,
    observation: &str,
    recording: &str,
    stated: Option<&str>,
    origin: IsrcOrigin,
) -> Result<()> {
    let Some(isrc) = stated.and_then(|value| hito_plugin_sdk::normalize_isrc(value.trim())) else {
        return Ok(());
    };
    sqlx::query("INSERT OR IGNORE INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES(?,?,?,?)")
        .bind(observation)
        .bind(recording)
        .bind(isrc)
        .bind(origin.name())
        .execute(connection)
        .await?;
    Ok(())
}
