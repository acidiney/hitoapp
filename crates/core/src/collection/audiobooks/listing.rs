use super::super::{Collection, Playlist};
use crate::{Actor, Result};
use std::time::Instant;
use uuid::Uuid;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_list", request_id = %Uuid::new_v4()))]
    pub async fn audiobooks(&self, actor: &Actor) -> Result<Vec<Playlist>> {
        let started = Instant::now();
        let result: Result<Vec<Playlist>> = async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let books = sqlx::query_as(
                "SELECT playlist.id,playlist.name,playlist.kind
                 FROM audiobooks book JOIN playlists playlist
                    ON playlist.id=book.playlist_id AND playlist.user_id=book.user_id
                 WHERE book.user_id=? AND playlist.deleted=0 AND playlist.kind='local'
                 ORDER BY playlist.name,playlist.id LIMIT 100",
            )
            .bind(actor.id())
            .fetch_all(&mut *tx)
            .await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(books)
        }
        .await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "read")
                .unwrap_or_else(|error| error.code()),
            book_count = result.as_ref().map(Vec::len).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook list completed"
        );
        result
    }
}
