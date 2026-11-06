use super::{Catalog, Recording, push_available, push_plays_from_services, push_primary_artist};
use crate::{Actor, Error, Result, metadata};
use sqlx::QueryBuilder;
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), count = recordings.len()))]
    pub async fn recordings_by_ids(
        &self,
        actor: &Actor,
        recordings: &[Uuid],
    ) -> Result<Vec<Recording>> {
        if recordings.len() > 1000 {
            return Err(Error::Invalid(
                "Select at most 1000 recordings per lookup".into(),
            ));
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            self.resolve_recordings(actor, recordings),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Recording selection could not be checked in time",
        )))
    }

    async fn resolve_recordings(
        &self,
        actor: &Actor,
        recordings: &[Uuid],
    ) -> Result<Vec<Recording>> {
        actor.check_session(&self.store).await?;
        let selected = recordings_on(
            &mut *self.store.pool.acquire().await?,
            &self.host,
            actor,
            recordings,
        )
        .await?;
        actor.check_session(&self.store).await?;
        Ok(selected)
    }
}

pub(crate) async fn recordings_on(
    connection: &mut sqlx::SqliteConnection,
    host: &crate::HostDeviceId,
    actor: &Actor,
    recordings: &[Uuid],
) -> Result<Vec<Recording>> {
    let requested = serde_json::to_string(recordings)
        .map_err(|_| Error::Internal("Recording selection could not be encoded".into()))?;
    let mut query = QueryBuilder::new("SELECT r.id, r.library_id, r.release_id, ");
    query
        .push(metadata::RECORDING_TITLE)
        .push(" AS title, r.title AS inherited_title, ")
        .push(metadata::RECORDING_ARTIST)
        .push(" AS artist, r.artist AS inherited_artist, a.title AS album, r.duration_ms, ");
    push_available(&mut query, host, actor.id());
    query.push(" AS available, ");
    push_primary_artist(&mut query, actor.id());
    query.push(" AS artist_id, ");
    push_plays_from_services(&mut query, actor.id());
    query.push(" AS plays_from_services, COALESCE(l.liked, 0) AS liked FROM json_each(")
            .push_bind(requested)
            .push(") requested CROSS JOIN recordings r ON r.id = requested.value
                JOIN library_members member ON member.library_id = r.library_id AND member.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" LEFT JOIN account_releases a ON a.id = r.release_id AND a.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" LEFT JOIN likes l ON l.recording_id = r.id AND l.user_id = ")
            .push_bind(actor.id().to_owned());
    metadata::join_recording_fields(&mut query, actor);
    query.push(" ORDER BY CAST(requested.key AS INTEGER)");
    let selected: Vec<Recording> = query.build_query_as().fetch_all(connection).await?;
    if selected.len() != recordings.len() {
        return Err(Error::NotFound);
    }
    Ok(selected)
}
