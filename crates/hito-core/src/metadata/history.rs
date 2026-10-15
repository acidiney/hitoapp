use super::{MetadataHistoryQuery, MetadataObservation, ObservationOrigin, ObservationPage};
use crate::{Error, Result};
use sqlx::{QueryBuilder, Row, Sqlite, Transaction, sqlite::SqliteRow};

pub(super) enum Target<'a> {
    Recording(&'a str),
    Release(&'a str),
}

pub(super) async fn page(
    tx: &mut Transaction<'_, Sqlite>,
    target: Target<'_>,
    page: MetadataHistoryQuery,
) -> Result<ObservationPage> {
    if page.before.is_some_and(|before| before <= 0) {
        return Err(Error::Invalid("Use a positive observation cursor".into()));
    }
    let limit = page.limit.unwrap_or(20).clamp(1, 100);
    let (column, id) = match target {
        Target::Recording(id) => ("recording_id", id),
        Target::Release(id) => ("release_id", id),
    };
    let mut query = QueryBuilder::new("SELECT * FROM metadata_observations WHERE ");
    query
        .push(column)
        .push(" = ")
        .push_bind(id.to_owned())
        .push(" AND sequence < ")
        .push_bind(page.before.unwrap_or(i64::MAX))
        .push(" ORDER BY sequence DESC LIMIT ")
        .push_bind(limit + 1);
    let mut rows = query.build().fetch_all(&mut **tx).await?;
    let next_before = if rows.len() > limit as usize {
        rows.pop();
        rows.last()
            .map(|row| row.try_get::<i64, _>("sequence"))
            .transpose()?
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(observation)
        .collect::<Result<Vec<_>>>()?;
    Ok(ObservationPage { items, next_before })
}

pub(super) fn observation(row: SqliteRow) -> Result<MetadataObservation> {
    let origin: &str = row.try_get("origin")?;
    let origin = match origin {
        "file_inspection" => ObservationOrigin::FileInspection,
        "legacy_normalized" => ObservationOrigin::LegacyNormalized,
        "provider_response" => ObservationOrigin::ProviderResponse,
        _ => return Err(Error::Internal("Metadata origin could not be read".into())),
    };
    let tags: Option<String> = row.try_get("tags")?;
    let tags = tags
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| Error::Internal("Observed tags could not be read".into()))?;
    Ok(MetadataObservation {
        id: row.try_get("id")?,
        source_id: row.try_get("source_id")?,
        release_id: row.try_get("release_id")?,
        provider: row.try_get("provider")?,
        origin,
        observed_at: row.try_get("observed_at")?,
        inspection_version: row.try_get("inspection_version")?,
        plugin_installation_id: row.try_get("plugin_installation_id")?,
        provider_recording_id: row.try_get("provider_recording_id")?,
        provider_revision: row.try_get("provider_revision")?,
        title: row.try_get("title")?,
        artist: row.try_get("artist")?,
        album: row.try_get("album")?,
        album_artist: row.try_get("album_artist")?,
        duration_ms: row.try_get("duration_ms")?,
        disc_number: row.try_get("disc_number")?,
        track_number: row.try_get("track_number")?,
        tags,
    })
}
