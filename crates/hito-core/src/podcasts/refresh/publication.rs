use super::{
    Lease,
    receive::{Content, ReceivedFeed},
    stored_number,
};
use crate::{
    Error, Result,
    podcasts::feed::{EpisodeIdentity, FeedSnapshot},
};
use uuid::Uuid;

pub(super) async fn commit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lease: &Lease,
    feed: ReceivedFeed,
) -> Result<()> {
    if let Content::Modified(snapshot) = feed.content {
        publish_episodes(transaction, lease, snapshot).await?;
    }
    sqlx::query("UPDATE podcast_subscriptions SET validator_url=?,etag=?,last_modified=?,fetch_url=coalesce(?,fetch_url),last_refreshed_at=unixepoch(),next_refresh_at=unixepoch()+3600,error=NULL WHERE id=? AND generation=?")
        .bind(feed.validators.representation_url()).bind(feed.validators.etag()).bind(feed.validators.last_modified())
        .bind(feed.permanent_url.as_deref()).bind(&lease.subscription_id).bind(lease.generation)
        .execute(&mut **transaction).await?;
    Ok(())
}

async fn publish_episodes(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lease: &Lease,
    snapshot: FeedSnapshot,
) -> Result<()> {
    for episode in snapshot.episodes {
        let (kind, value) = match episode.identity {
            EpisodeIdentity::Guid(value) => ("guid", value),
            EpisodeIdentity::Enclosure(value) => ("enclosure", value),
        };
        let existing: Option<String> = sqlx::query_scalar("SELECT recording_id FROM podcast_episodes WHERE subscription_id=? AND identity_kind=? AND identity_value=?")
            .bind(&lease.subscription_id).bind(kind).bind(&value).fetch_optional(&mut **transaction).await?;
        let artist = episode
            .author
            .as_deref()
            .or(snapshot.author.as_deref())
            .unwrap_or(&snapshot.title);
        let duration = stored_number(episode.duration_ms.unwrap_or(0))?;
        let length = episode.length.map(stored_number).transpose()?;
        let recording = if let Some(existing) = existing {
            sqlx::query(
                "UPDATE recordings SET title=?,artist=?,duration_ms=? WHERE id=? AND library_id=?",
            )
            .bind(&episode.title)
            .bind(artist)
            .bind(duration)
            .bind(&existing)
            .bind(&lease.library_id)
            .execute(&mut **transaction)
            .await?;
            existing
        } else {
            let recording = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO recordings(id,library_id,title,artist,duration_ms,provenance) VALUES(?,?,?,?,?,'rss_feed')")
                .bind(&recording).bind(&lease.library_id).bind(&episode.title).bind(artist).bind(duration).execute(&mut **transaction).await?;
            recording
        };
        crate::metadata::refresh_recording_credits(transaction, &recording).await?;
        let episode_id: String = sqlx::query_scalar("INSERT INTO podcast_episodes(id,subscription_id,user_id,recording_id,identity_kind,identity_value,enclosure_url,declared_mime,byte_length,description,published_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(subscription_id,identity_kind,identity_value) DO UPDATE SET enclosure_revision=podcast_episodes.enclosure_revision+CASE WHEN podcast_episodes.enclosure_url IS NOT excluded.enclosure_url OR podcast_episodes.declared_mime IS NOT excluded.declared_mime OR podcast_episodes.byte_length IS NOT excluded.byte_length THEN 1 ELSE 0 END,enclosure_url=excluded.enclosure_url,declared_mime=excluded.declared_mime,byte_length=excluded.byte_length,description=excluded.description,published_at=excluded.published_at,last_seen_at=unixepoch() RETURNING id")
            .bind(Uuid::new_v4().to_string()).bind(&lease.subscription_id).bind(&lease.user_id).bind(&recording).bind(kind).bind(value)
            .bind(episode.enclosure_url).bind(episode.declared_mime).bind(length).bind(episode.description).bind(episode.published_at).fetch_one(&mut **transaction).await?;
        let source: Option<String> =
            sqlx::query_scalar("SELECT source_id FROM podcast_sources WHERE episode_id=?")
                .bind(&episode_id)
                .fetch_optional(&mut **transaction)
                .await?;
        if let Some(source) = source {
            let retained = sqlx::query("UPDATE sources SET ready=1 WHERE id=? AND recording_id=? AND provider='podcast' AND provider_id=? AND device_id IS NULL")
                .bind(source).bind(&recording).bind(&episode_id).execute(&mut **transaction).await?;
            if retained.rows_affected() != 1 {
                return Err(Error::Internal("Podcast source identity changed".into()));
            }
        } else {
            let source = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready) VALUES(?,?,'podcast',?,NULL,1)")
                .bind(&source).bind(&recording).bind(&episode_id).execute(&mut **transaction).await?;
            sqlx::query("INSERT INTO podcast_sources(episode_id,source_id) VALUES(?,?)")
                .bind(&episode_id)
                .bind(source)
                .execute(&mut **transaction)
                .await?;
        }
    }
    sqlx::query("UPDATE podcast_subscriptions SET title=?,author=?,description=?,content_revision=content_revision+1 WHERE id=? AND generation=?")
        .bind(snapshot.title).bind(snapshot.author).bind(snapshot.description).bind(&lease.subscription_id).bind(lease.generation).execute(&mut **transaction).await?;
    Ok(())
}
