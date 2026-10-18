mod admission;
mod bookmarks;
mod continuation;
mod listing;
mod playback;
mod playback_admission;
pub use playback_admission::AudiobookPlaybackAdmission;
mod types;

use super::{Collection, Playlist, apply::playlists::editable, playlist_entries, playlists};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

pub(super) use bookmarks::{
    Create as BookmarkCreate, create as create_bookmark, remove as remove_bookmark,
};
pub(super) use continuation::set as set_chapter;
pub use types::{
    AudiobookBookmark, AudiobookChapterAnchor, AudiobookContinuation, AudiobookDetail,
    AudiobookPlayback,
};

const MAX_BOOKMARKS: i64 = 256;
const MAX_CHAPTERS: i64 = 10_000;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_read", request_id = %Uuid::new_v4()))]
    pub async fn audiobook(&self, actor: &Actor, id: &str) -> Result<AudiobookDetail> {
        let started = Instant::now();
        let result: Result<AudiobookDetail> = async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let playlist = owned_on(&mut tx, actor, id).await?;
            check_chapter_limit(&mut tx, id).await?;
            let chapters = playlist_entries::read(
                &mut tx,
                &self.host,
                actor,
                id,
                playlist_entries::EntryAccess::All,
            )
            .await?;
            let bookmarks = sqlx::query_as::<_, AudiobookBookmark>(
                "SELECT bookmark.id,bookmark.audiobook_id,bookmark.entry_id,
                    bookmark.recording_id,bookmark.position_ms,bookmark.label
                 FROM audiobook_bookmarks bookmark JOIN playlist_entries chapter
                    ON chapter.playlist_id=bookmark.audiobook_id AND chapter.id=bookmark.entry_id
                 WHERE bookmark.user_id=? AND bookmark.audiobook_id=? AND bookmark.deleted=0
                 ORDER BY chapter.position,chapter.id,bookmark.position_ms,bookmark.id LIMIT ?",
            )
            .bind(actor.id())
            .bind(id)
            .bind(MAX_BOOKMARKS + 1)
            .fetch_all(&mut *tx)
            .await?;
            if bookmarks.len() > MAX_BOOKMARKS as usize {
                return Err(Error::Internal(
                    "The audiobook bookmark limit was exceeded".into(),
                ));
            }
            let continuation = continuation::read(&mut tx, actor, id).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(AudiobookDetail {
                playlist,
                chapters,
                bookmarks,
                continuation,
            })
        }
        .await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "read")
                .unwrap_or_else(|error| error.code()),
            chapter_count = result.as_ref().map(|book| book.chapters.len()).unwrap_or(0),
            bookmark_count = result
                .as_ref()
                .map(|book| book.bookmarks.len())
                .unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Audiobook read completed"
        );
        result
    }
}

#[tracing::instrument(level = "debug", skip_all, fields(operation = "audiobook_create", request_id = %operation_id))]
pub(super) async fn create(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    playlist: &Uuid,
    operation_id: Uuid,
) -> Result<()> {
    let started = Instant::now();
    let result: Result<()> = async {
        if !editable(tx, actor, playlist).await? {
            return Ok(());
        }
        let id = playlist.to_string();
        local_playlist_on(tx, actor, &id).await?;
        check_chapter_limit(tx, &id).await?;
        sqlx::query("INSERT INTO audiobooks(playlist_id,user_id) VALUES(?,?) ON CONFLICT(playlist_id) DO NOTHING")
            .bind(id).bind(actor.id()).execute(&mut **tx).await?;
        Ok(())
    }.await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Audiobook grouping completed"
    );
    result
}

async fn local_playlist_on(
    connection: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<Playlist> {
    let playlist = playlists::read_owned(connection, actor, id).await?;
    if playlist.kind != "local" {
        return Err(Error::Conflict(
            "Only an editable playlist can become an audiobook".into(),
        ));
    }
    Ok(playlist)
}

async fn check_chapter_limit(connection: &mut SqliteConnection, id: &str) -> Result<()> {
    let overflow: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM playlist_entries WHERE playlist_id=? AND deleted=0 LIMIT 1 OFFSET ?)")
        .bind(id).bind(MAX_CHAPTERS).fetch_one(connection).await?;
    if overflow {
        return Err(Error::Invalid(
            "Audiobooks support at most 10,000 chapters".into(),
        ));
    }
    Ok(())
}

async fn owned_on(connection: &mut SqliteConnection, actor: &Actor, id: &str) -> Result<Playlist> {
    let playlist = local_playlist_on(connection, actor, id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM audiobooks WHERE playlist_id=? AND user_id=?)",
    )
    .bind(id)
    .bind(actor.id())
    .fetch_one(connection)
    .await?;
    if !exists {
        return Err(Error::NotFound);
    }
    Ok(playlist)
}
