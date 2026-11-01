use crate::Result;
use sqlx::SqliteConnection;
use uuid::Uuid;

const TAGS: [(&str, &str); 4] = [
    ("release", "musicbrainz_album_id"),
    ("release_group", "musicbrainz_release_group_id"),
    ("recording", "musicbrainz_recording_id"),
    ("release_track", "musicbrainz_release_track_id"),
];

pub(crate) async fn retain_tag_identities(
    connection: &mut SqliteConnection,
    observation: &str,
    recording: &str,
    release: Option<&str>,
    tags: Option<&str>,
) -> Result<()> {
    let Some(tags) = tags.and_then(|tags| serde_json::from_str::<serde_json::Value>(tags).ok())
    else {
        return Ok(());
    };
    for (entity, field) in TAGS {
        let Some(identifier) = tags[field]["value"].as_str().and_then(canonical) else {
            continue;
        };
        sqlx::query("INSERT OR IGNORE INTO recording_musicbrainz_observations(observation_id,entity,mbid,recording_id,release_id) VALUES(?,?,?,?,?)")
            .bind(observation)
            .bind(entity)
            .bind(identifier)
            .bind(recording)
            .bind(release)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

fn canonical(value: &str) -> Option<String> {
    let value = value.trim();
    let parsed = Uuid::try_parse(value).ok().filter(|id| !id.is_nil())?;
    (value.len() == 36).then(|| parsed.hyphenated().to_string())
}
