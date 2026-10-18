use super::super::{Collection, Playlist};
use super::listing_cursor::{Cursor, Direction};
use super::{AudiobookLibraryPage, AudiobookLibraryQuery, AudiobookPagination};
use crate::{Actor, Error, Result};
use std::time::Instant;
use uuid::Uuid;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_library", request_id = %Uuid::new_v4()))]
    pub async fn audiobook_library(
        &self,
        actor: &Actor,
        query: AudiobookLibraryQuery,
    ) -> Result<AudiobookLibraryPage> {
        let started = Instant::now();
        let result = self.read_audiobook_page(actor, query).await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "read")
                .unwrap_or_else(|error| error.code()),
            book_count = result.as_ref().map(|page| page.items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook library page completed"
        );
        result
    }

    async fn read_audiobook_page(
        &self,
        actor: &Actor,
        query: AudiobookLibraryQuery,
    ) -> Result<AudiobookLibraryPage> {
        let limit = query.limit.unwrap_or(60);
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid(
                "Audiobook pages contain 1 to 100 books".into(),
            ));
        }
        let cursor = query.cursor.as_deref().map(Cursor::decode).transpose()?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(cursor) = &cursor {
            if cursor.owner != actor.id() {
                return Err(Error::NotFound);
            }
            let name: Option<String> = sqlx::query_scalar(
                "SELECT playlist.name FROM audiobooks book INDEXED BY sqlite_autoindex_audiobooks_1 CROSS JOIN playlists playlist INDEXED BY sqlite_autoindex_playlists_1
                 ON playlist.id=book.playlist_id AND playlist.user_id=book.user_id
                 WHERE book.user_id=? AND book.playlist_id=? AND playlist.deleted=0 AND playlist.kind='local'")
                .bind(actor.id()).bind(cursor.playlist.to_string()).fetch_optional(&mut *tx).await?;
            if name.as_deref() != Some(cursor.name.as_str()) {
                return Err(Error::Conflict(
                    "The audiobook list changed. Restart browsing audiobooks.".into(),
                ));
            }
        }
        let previous = cursor
            .as_ref()
            .is_some_and(|cursor| matches!(cursor.direction, Direction::Previous));
        let filter = match &cursor {
            None => "",
            Some(_) if previous => " AND (playlist.name,playlist.id)<(?,?)",
            Some(_) => " AND (playlist.name,playlist.id)>(?,?)",
        };
        let order = if previous {
            " ORDER BY playlist.name DESC,playlist.id DESC LIMIT ?"
        } else {
            " ORDER BY playlist.name,playlist.id LIMIT ?"
        };
        let sql = format!("SELECT playlist.id,playlist.name,playlist.kind
            FROM audiobooks book INDEXED BY audiobooks_account CROSS JOIN playlists playlist INDEXED BY sqlite_autoindex_playlists_1
                ON playlist.id=book.playlist_id AND playlist.user_id=book.user_id
            WHERE book.user_id=? AND playlist.deleted=0 AND playlist.kind='local'{filter}{order}");
        let mut read = sqlx::query_as::<_, Playlist>(&sql).bind(actor.id());
        if let Some(cursor) = &cursor {
            read = read.bind(&cursor.name).bind(cursor.playlist.to_string());
        }
        let mut items = read.bind(i64::from(limit) + 1).fetch_all(&mut *tx).await?;
        if cursor.is_some() && items.is_empty() {
            return Err(Error::Conflict(
                "The audiobook list changed. Restart browsing audiobooks.".into(),
            ));
        }
        let more = items.len() > usize::from(limit);
        items.truncate(usize::from(limit));
        if previous {
            items.reverse();
        }
        let (next, previous) = if previous {
            (true, more)
        } else {
            (more, cursor.is_some())
        };
        let encode = |item: &Playlist, direction| {
            Cursor::encode(actor.id(), &item.name, &item.id, direction)
        };
        let next_cursor = if next {
            items
                .last()
                .map(|item| encode(item, Direction::Next))
                .transpose()?
        } else {
            None
        };
        let previous_cursor = if previous {
            items
                .first()
                .map(|item| encode(item, Direction::Previous))
                .transpose()?
        } else {
            None
        };
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        Ok(AudiobookLibraryPage {
            items,
            pagination: AudiobookPagination {
                limit,
                next_cursor,
                previous_cursor,
            },
        })
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_grouping_choices", request_id = %Uuid::new_v4()))]
    pub async fn audiobook_grouping_playlists(&self, actor: &Actor) -> Result<Vec<Playlist>> {
        let started = Instant::now();
        let result: Result<Vec<Playlist>> = async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let choices = sqlx::query_as(
                "SELECT playlist.id,playlist.name,playlist.kind FROM (
                    SELECT id,name,kind FROM playlists WHERE user_id=? AND deleted=0 ORDER BY name,id LIMIT 1000
                 ) playlist WHERE playlist.kind='local' AND NOT EXISTS(
                    SELECT 1 FROM audiobooks book WHERE book.user_id=? AND book.playlist_id=playlist.id)
                 ORDER BY playlist.name,playlist.id")
                .bind(actor.id()).bind(actor.id()).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(choices)
        }.await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "read")
                .unwrap_or_else(|error| error.code()),
            choice_count = result.as_ref().map(Vec::len).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook grouping choices completed"
        );
        result
    }
}
