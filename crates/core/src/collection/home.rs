use super::{AddedAlbum, Collection, Home, HomeAlbum, UnheardOnAlbum};
use crate::{
    Actor, HostDeviceId, Recording, Result,
    catalog::{push_available, push_plays_from_services},
    metadata,
};
use sqlx::QueryBuilder;
use std::time::Instant;

const RECENT_PLAYS: i64 = 200;
const RECENT_ALBUMS: i64 = 12;
const ADDED_ALBUMS: i64 = 12;
const UNHEARD_ALBUMS: usize = 3;
const UNHEARD_TRACKS: i64 = 25;
const UNHEARD_COUNT_CAP: i64 = 1_000;
const LIKED_COUNT_CAP: i64 = 1_000;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn home(&self, actor: &Actor) -> Result<Home> {
        let started = Instant::now();
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let has_music: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM library_members m
                JOIN recordings r ON r.library_id = m.library_id
                WHERE m.user_id = ?
                  AND NOT EXISTS (SELECT 1 FROM podcast_episodes e WHERE e.recording_id = r.id))",
        )
        .bind(actor.id())
        .fetch_one(&mut *tx)
        .await?;
        let liked_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM (
                SELECT 1 FROM likes l JOIN recordings r ON r.id = l.recording_id
                WHERE l.user_id = ? AND l.liked = 1
                  AND EXISTS (SELECT 1 FROM library_members m WHERE m.library_id = r.library_id AND m.user_id = l.user_id)
                LIMIT ?)",
        )
        .bind(actor.id())
        .bind(LIKED_COUNT_CAP)
        .fetch_one(&mut *tx)
        .await?;
        let mut recent_albums: Vec<HomeAlbum> = sqlx::query_as(
            "SELECT r.release_id AS id, a.title, a.artist,
                    MAX(p.updated_at) AS last_played_at, COUNT(*) AS played
             FROM (SELECT recording_id, updated_at FROM listening_progress
                   WHERE user_id = ? ORDER BY updated_at DESC, rowid LIMIT ?) p
             JOIN recordings r ON r.id = p.recording_id
             JOIN account_releases a ON a.user_id = ? AND a.id = r.release_id
             WHERE EXISTS (SELECT 1 FROM library_members m WHERE m.library_id = r.library_id AND m.user_id = ?)
             GROUP BY r.release_id
             ORDER BY last_played_at DESC, r.release_id LIMIT ?",
        )
        .bind(actor.id())
        .bind(RECENT_PLAYS)
        .bind(actor.id())
        .bind(actor.id())
        .bind(RECENT_ALBUMS)
        .fetch_all(&mut *tx)
        .await?;
        let mut recently_added: Vec<AddedAlbum> = sqlx::query_as(
            "SELECT a.id, a.title, a.artist FROM account_releases a
             WHERE a.user_id = ?
               AND EXISTS (SELECT 1 FROM library_members m WHERE m.library_id = a.library_id AND m.user_id = a.user_id)
               AND EXISTS (SELECT 1 FROM recordings r WHERE r.release_id = a.id
                           AND NOT EXISTS (SELECT 1 FROM podcast_episodes e WHERE e.recording_id = r.id))
             ORDER BY a.rowid DESC LIMIT ?",
        )
        .bind(actor.id())
        .bind(ADDED_ALBUMS)
        .fetch_all(&mut *tx)
        .await?;
        let ids: Vec<String> = recent_albums
            .iter()
            .map(|album| album.id.clone())
            .chain(recently_added.iter().map(|album| album.id.clone()))
            .collect();
        let mut inferred: std::collections::HashMap<String, crate::AlbumArtistInference> =
            crate::catalog::releases::releases_among_ids_on(&mut tx, actor, &ids)
                .await?
                .into_iter()
                .filter_map(|release| Some((release.id, release.inferred_artist?)))
                .collect();
        for album in &mut recent_albums {
            album.inferred_artist = inferred.get(&album.id).cloned();
        }
        for album in &mut recently_added {
            album.inferred_artist = inferred.remove(&album.id);
        }
        let mut unheard = Vec::new();
        for album in &recent_albums {
            if unheard.len() == UNHEARD_ALBUMS {
                break;
            }
            let tracks = self
                .unheard_tracks(&mut tx, actor, &album.id, UNHEARD_TRACKS)
                .await?;
            if tracks.is_empty() {
                continue;
            }
            let remaining = self.unheard_count(&mut tx, actor, &album.id).await?;
            unheard.push(UnheardOnAlbum {
                release_id: album.id.clone(),
                title: album.title.clone(),
                artist: album.artist.clone(),
                inferred_artist: album.inferred_artist.clone(),
                played: album.played,
                remaining,
                tracks,
            });
        }
        tx.commit().await?;
        tracing::debug!(
            recent_albums = recent_albums.len(),
            unheard_albums = unheard.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Home summary read"
        );
        Ok(Home {
            has_music,
            liked_count,
            recent_albums,
            recently_added,
            unheard,
        })
    }

    async fn unheard_count(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        actor: &Actor,
        release: &str,
    ) -> Result<i64> {
        let mut query = QueryBuilder::new(
            "SELECT count(*) FROM (SELECT 1 FROM recordings r WHERE r.release_id = ",
        );
        query.push_bind(release.to_owned());
        push_unheard_filters(&mut query, &self.host, actor);
        query.push(" LIMIT ").push_bind(UNHEARD_COUNT_CAP).push(")");
        Ok(query.build_query_scalar().fetch_one(&mut **tx).await?)
    }

    async fn unheard_tracks(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        actor: &Actor,
        release: &str,
        limit: i64,
    ) -> Result<Vec<Recording>> {
        let mut query = QueryBuilder::new("SELECT r.id, r.library_id, r.release_id, ");
        query
            .push(metadata::RECORDING_TITLE)
            .push(" AS title, r.title AS inherited_title, ")
            .push(metadata::RECORDING_ARTIST)
            .push(" AS artist, r.artist AS inherited_artist, a.title AS album, r.duration_ms, ");
        push_available(&mut query, &self.host, actor.id());
        query.push(" AS available, ");
        push_plays_from_services(&mut query, actor.id());
        query
            .push(" AS plays_from_services, COALESCE(l.liked, 0) AS liked FROM recordings r LEFT JOIN account_releases a ON a.id = r.release_id AND a.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" LEFT JOIN likes l ON l.recording_id = r.id AND l.user_id = ")
            .push_bind(actor.id().to_owned());
        metadata::join_recording_fields(&mut query, actor);
        query
            .push(" WHERE r.release_id = ")
            .push_bind(release.to_owned());
        push_unheard_filters(&mut query, &self.host, actor);
        query
            .push(
                " ORDER BY (SELECT min(COALESCE(sm.disc_number, 1)) FROM sources ss
                    JOIN source_metadata sm ON sm.source_id = ss.id
                    WHERE ss.recording_id = r.id AND sm.release_id = r.release_id),
                  (SELECT min(sm.track_number) FROM sources ss
                    JOIN source_metadata sm ON sm.source_id = ss.id
                    WHERE ss.recording_id = r.id AND sm.release_id = r.release_id) IS NULL,
                  (SELECT min(sm.track_number) FROM sources ss
                    JOIN source_metadata sm ON sm.source_id = ss.id
                    WHERE ss.recording_id = r.id AND sm.release_id = r.release_id),
                  title COLLATE NOCASE, r.id LIMIT ",
            )
            .push_bind(limit);
        Ok(query.build_query_as().fetch_all(&mut **tx).await?)
    }
}

fn push_unheard_filters(
    query: &mut QueryBuilder<'_, sqlx::Sqlite>,
    host: &HostDeviceId,
    actor: &Actor,
) {
    query
        .push(" AND EXISTS (SELECT 1 FROM library_members m WHERE m.library_id = r.library_id AND m.user_id = ")
        .push_bind(actor.id().to_owned())
        .push(") AND NOT EXISTS (SELECT 1 FROM listening_progress p WHERE p.recording_id = r.id AND p.user_id = ")
        .push_bind(actor.id().to_owned())
        .push(") AND ");
    push_available(query, host, actor.id());
}
