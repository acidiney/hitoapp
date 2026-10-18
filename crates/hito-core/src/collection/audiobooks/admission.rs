use super::{AudiobookChapterAnchor, Collection};
use crate::{Actor, Error, Result};
use sqlx::QueryBuilder;
use std::time::{Duration, Instant};
use uuid::Uuid;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_chapters_check", request_id = %Uuid::new_v4(), chapter_count = chapters.len()))]
    pub async fn check_audiobook_chapters(
        &self,
        actor: &Actor,
        chapters: &[AudiobookChapterAnchor],
    ) -> Result<()> {
        let started = Instant::now();
        let result: Result<()> = tokio::time::timeout(Duration::from_secs(1), async {
            if chapters.len() > 1000
                || chapters.iter().any(|chapter| {
                    chapter.audiobook_id.is_nil()
                        || chapter.entry_id.is_nil()
                        || chapter.recording_id.is_nil()
                })
            {
                return Err(Error::Invalid(
                    "Select at most 1000 nonempty audiobook chapter identities".into(),
                ));
            }
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            if !chapters.is_empty() {
                let mut query = QueryBuilder::new(
                    "WITH requested(audiobook_id,entry_id,recording_id) AS (",
                );
                query.push_values(chapters, |mut row, chapter| {
                    row.push_bind(chapter.audiobook_id.to_string())
                        .push_bind(chapter.entry_id.to_string())
                        .push_bind(chapter.recording_id.to_string());
                });
                query.push(
                    ") SELECT NOT EXISTS(SELECT 1 FROM requested WHERE NOT EXISTS(
                        SELECT 1 FROM playlists playlist INDEXED BY sqlite_autoindex_playlists_1
                        CROSS JOIN audiobooks book INDEXED BY sqlite_autoindex_audiobooks_1
                            ON book.playlist_id=playlist.id AND book.user_id=playlist.user_id
                        CROSS JOIN playlist_entries chapter INDEXED BY sqlite_autoindex_playlist_entries_1
                            ON chapter.playlist_id=playlist.id AND chapter.id=requested.entry_id
                        CROSS JOIN recordings recording INDEXED BY sqlite_autoindex_recordings_1
                            ON recording.id=chapter.recording_id
                        CROSS JOIN library_members access INDEXED BY sqlite_autoindex_library_members_1
                            ON access.library_id=recording.library_id AND access.user_id=playlist.user_id
                        WHERE playlist.id=requested.audiobook_id AND playlist.user_id=",
                );
                query.push_bind(actor.id()).push(
                    " AND playlist.deleted=0 AND playlist.kind='local' AND chapter.deleted=0
                        AND chapter.recording_id=requested.recording_id))",
                );
                let valid: bool = query.build_query_scalar().fetch_one(&mut *tx).await?;
                if !valid {
                    return Err(Error::NotFound);
                }
            }
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(())
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Audiobook chapters could not be checked in time",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "accepted")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook chapter admission completed"
        );
        result
    }
}
