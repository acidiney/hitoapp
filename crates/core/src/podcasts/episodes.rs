use super::{
    EpisodesWithProgress, PodcastEpisode, PodcastEpisodePage, PodcastEpisodeQuery, Podcasts,
    subscriptions::identity,
};
use crate::{Actor, Error, Recording, Result, catalog::recordings_on, collection::saved_progress};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_CURSOR_BYTES: usize = 2048;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    owner: String,
    subscription: Uuid,
    revision: i64,
    sort_key: i64,
    episode: Uuid,
}

#[derive(sqlx::FromRow)]
struct EpisodeRow {
    id: String,
    recording_id: String,
    sort_key: i64,
}

impl Podcasts {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn episodes(
        &self,
        actor: &Actor,
        subscription: &str,
        query: PodcastEpisodeQuery,
    ) -> Result<PodcastEpisodePage> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let subscription_id = Uuid::parse_str(subscription).map_err(|_| Error::NotFound)?;
        let subscription = subscription_id.to_string();
        let (total, revision): (i64, i64) = sqlx::query_as(
            "SELECT p.episode_count,p.content_revision FROM podcast_subscriptions p
             JOIN current_podcast_authority a ON a.id=p.id WHERE p.id=? AND p.user_id=?",
        )
        .bind(&subscription)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let cursor = query.after.as_deref().map(decode_cursor).transpose()?;
        if let Some(cursor) = &cursor {
            if cursor.owner != actor.id() || cursor.subscription != subscription_id {
                return Err(Error::NotFound);
            }
            if cursor.revision != revision {
                return Err(stale());
            }
            let anchor: Option<i64> = sqlx::query_scalar(
                "SELECT sort_key FROM podcast_episodes WHERE id=? AND subscription_id=? AND user_id=?",
            ).bind(cursor.episode.to_string()).bind(&subscription).bind(actor.id()).fetch_optional(&mut *tx).await?;
            if anchor != Some(cursor.sort_key) {
                return Err(invalid_cursor());
            }
        }
        let limit = query.limit.unwrap_or(60).clamp(1, 200);
        let mut rows: Vec<EpisodeRow> = if let Some(cursor) = cursor {
            sqlx::query_as(
                "SELECT id,recording_id,sort_key FROM podcast_episodes
                 WHERE subscription_id=? AND user_id=? AND (sort_key,id)<(?,?)
                 ORDER BY sort_key DESC,id DESC LIMIT ?",
            )
            .bind(&subscription)
            .bind(actor.id())
            .bind(cursor.sort_key)
            .bind(cursor.episode.to_string())
            .bind(i64::from(limit) + 1)
            .fetch_all(&mut *tx)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id,recording_id,sort_key FROM podcast_episodes
                 WHERE subscription_id=? AND user_id=? ORDER BY sort_key DESC,id DESC LIMIT ?",
            )
            .bind(&subscription)
            .bind(actor.id())
            .bind(i64::from(limit) + 1)
            .fetch_all(&mut *tx)
            .await?
        };
        let has_more = rows.len() > usize::from(limit);
        rows.truncate(usize::from(limit));
        let next_cursor = if has_more {
            let last = rows
                .last()
                .ok_or_else(|| Error::Internal("Podcast page is empty".into()))?;
            Some(encode_cursor(Cursor {
                version: 1,
                owner: actor.id().to_owned(),
                subscription: subscription_id,
                revision,
                sort_key: last.sort_key,
                episode: identity(&last.id)?,
            })?)
        } else {
            None
        };
        tx.commit().await?;
        let recording_ids = rows
            .iter()
            .map(|row| identity(&row.recording_id))
            .collect::<Result<Vec<_>>>()?;
        let recordings = self.episode_recordings(actor, &recording_ids).await?;
        let items = rows
            .into_iter()
            .zip(recordings)
            .map(|(row, recording)| {
                Ok(PodcastEpisode {
                    episode_id: identity(&row.id)?,
                    recording,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        actor.check_session(&self.store).await?;
        Ok(PodcastEpisodePage {
            items,
            total,
            limit,
            next_cursor,
            content_revision: revision,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), count = recordings.len()))]
    async fn episode_recordings(
        &self,
        actor: &Actor,
        recordings: &[Uuid],
    ) -> Result<Vec<Recording>> {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
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
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Recording selection could not be checked in time",
        )))
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn episodes_with_progress(
        &self,
        actor: &Actor,
        subscription: &str,
        query: PodcastEpisodeQuery,
    ) -> Result<EpisodesWithProgress> {
        let episodes = self.episodes(actor, subscription, query).await?;
        let recording_ids = episodes
            .items
            .iter()
            .map(|episode| {
                Uuid::parse_str(&episode.recording.id)
                    .map_err(|_| Error::Internal("Stored recording identity is invalid".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let progress = saved_progress(&self.store, actor, &recording_ids).await?;
        tracing::debug!(
            episode_count = episodes.items.len(),
            progress_count = progress.len(),
            "Podcast episodes joined with saved progress"
        );
        Ok(EpisodesWithProgress { episodes, progress })
    }
}

fn decode_cursor(value: &str) -> Result<Cursor> {
    if value.is_empty() || value.len() > MAX_CURSOR_BYTES {
        return Err(invalid_cursor());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| invalid_cursor())?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if cursor.version != 1 || cursor.owner.is_empty() || cursor.revision < 0 {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn encode_cursor(cursor: Cursor) -> Result<String> {
    let bytes = serde_json::to_vec(&cursor)
        .map_err(|_| Error::Internal("Podcast cursor could not be encoded".into()))?;
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    if encoded.len() > MAX_CURSOR_BYTES {
        return Err(Error::Internal(
            "Podcast cursor exceeds its size limit".into(),
        ));
    }
    Ok(encoded)
}

fn invalid_cursor() -> Error {
    Error::Invalid("This podcast episode cursor is invalid".into())
}

fn stale() -> Error {
    Error::Conflict("The podcast archive changed. Restart episode browsing.".into())
}
