use super::{Artwork, Catalog, rendition::ValidatedArtwork, target::Target};
use crate::{Actor, Error, Result, identity::recording_access};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

impl Catalog {
    pub(super) async fn retained_artwork(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<Option<Artwork>> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let hash: Option<String> = sqlx::query_scalar(
            "SELECT retained.content_hash FROM recording_artwork retained
             JOIN recordings r ON r.id = retained.recording_id
             JOIN libraries l ON l.id = r.library_id AND l.owner_id = retained.user_id
             WHERE retained.user_id = ? AND retained.recording_id = ?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_optional(&mut *tx)
        .await?;
        let artwork = match hash {
            Some(hash) => Some(ready_asset(&mut tx, actor.id(), &hash).await?),
            None => None,
        };
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::debug!(found = artwork.is_some(), "Retained recording artwork read");
        Ok(artwork)
    }
}

pub(super) async fn ready_asset(
    connection: &mut SqliteConnection,
    account: &str,
    hash: &str,
) -> Result<Artwork> {
    let asset: (String, Vec<u8>) = sqlx::query_as(
        "SELECT content_type,bytes FROM artwork_assets WHERE user_id = ? AND content_hash = ?",
    )
    .bind(account)
    .bind(hash)
    .fetch_one(connection)
    .await?;
    stored(asset)
}

pub(super) fn stored((content_type, bytes): (String, Vec<u8>)) -> Result<Artwork> {
    let content_type = match content_type.as_str() {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        _ => return Err(Error::Internal("Stored artwork type is invalid".into())),
    };
    Ok(Artwork {
        content_type,
        bytes,
    })
}

pub(super) async fn store(
    connection: &mut SqliteConnection,
    account: &str,
    image: ValidatedArtwork,
) -> Result<String> {
    sqlx::query("INSERT INTO artwork_assets(user_id,content_hash,content_type,width,height,bytes) VALUES (?,?,?,?,?,?) ON CONFLICT(user_id,content_hash) DO NOTHING")
        .bind(account).bind(&image.content_hash).bind(image.content_type).bind(i64::from(image.width))
        .bind(i64::from(image.height)).bind(&image.bytes).execute(&mut *connection).await?;
    Ok(image.content_hash)
}

pub(super) async fn publish(
    connection: &mut SqliteConnection,
    account: &str,
    target: &Target,
    fetch: &str,
    image: ValidatedArtwork,
) -> Result<()> {
    let hash = store(&mut *connection, account, image).await?;
    let retained = match target {
        Target::Recording { id, observation_id, .. } => sqlx::query(
            "INSERT INTO recording_artwork(user_id,recording_id,observation_id,fetch_id,content_hash) VALUES (?,?,?,?,?) ON CONFLICT(user_id,recording_id) DO UPDATE SET observation_id = excluded.observation_id,fetch_id = excluded.fetch_id,content_hash = excluded.content_hash",
        ).bind(account).bind(id).bind(observation_id),
        Target::Release { id, observation_id, .. } => sqlx::query(
            "INSERT INTO release_artwork(user_id,release_id,observation_id,fetch_id,content_hash) VALUES (?,?,?,?,?) ON CONFLICT(user_id,release_id) DO UPDATE SET observation_id = excluded.observation_id,fetch_id = excluded.fetch_id,content_hash = excluded.content_hash",
        ).bind(account).bind(id).bind(observation_id),
        Target::Playlist { id, .. } => sqlx::query(
            "INSERT INTO playlist_artwork(user_id,playlist_id,fetch_id,content_hash) VALUES (?,?,?,?) ON CONFLICT(user_id,playlist_id) DO UPDATE SET fetch_id = excluded.fetch_id,content_hash = excluded.content_hash",
        ).bind(account).bind(id),
    };
    retained
        .bind(fetch)
        .bind(&hash)
        .execute(&mut *connection)
        .await?;
    prune(connection, Some(account)).await?;
    Ok(())
}

pub(super) async fn prune(connection: &mut SqliteConnection, account: Option<&str>) -> Result<()> {
    let mut query: QueryBuilder<Sqlite> = QueryBuilder::new("DELETE FROM artwork_assets WHERE ");
    if let Some(account) = account {
        query
            .push("user_id = ")
            .push_bind(account.to_owned())
            .push(" AND ");
    }
    query.push(
        "NOT EXISTS(SELECT 1 FROM recording_artwork INDEXED BY recording_artwork_asset
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)
        AND NOT EXISTS(SELECT 1 FROM release_artwork INDEXED BY release_artwork_asset
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)
        AND NOT EXISTS(SELECT 1 FROM playlist_artwork INDEXED BY playlist_artwork_asset
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)
        AND NOT EXISTS(SELECT 1 FROM cover_art_lookups
            WHERE user_id = artwork_assets.user_id AND content_hash = artwork_assets.content_hash)",
    );
    query.build().execute(connection).await?;
    Ok(())
}
