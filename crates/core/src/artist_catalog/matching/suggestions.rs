use super::{
    ArtistMatchSuggestion, ArtistMatchSuggestions, DismissArtistMatchSuggestion,
    RestoreArtistMatchSuggestion,
};
use crate::artist_catalog::{
    LINKED, REQUEST_BUDGET,
    identity::groups::{current_choices, group_summary},
};
use crate::{Actor, ArtistCatalog, Error, Result};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, Transaction};
use std::{collections::HashSet, time::Instant};
use uuid::Uuid;

const MAX_GROUPS: i64 = 50;
const MAX_CREDIT_RECORDINGS: i64 = 500;
const MAX_GROUP_RECORDINGS: i64 = 500;
const LENGTH_TOLERANCE_MS: i64 = 3000;
const MAX_ID_LENGTH: usize = 64;

#[derive(sqlx::FromRow)]
struct Facts {
    shared_albums: i64,
    same_length_tracks: i64,
    undecided_tracks: i64,
}

impl ArtistCatalog {
    /// Ranks chosen artists whose members carry this credit's name. Equal names alone never
    /// change anything: the account accepts through the existing reviewed apply, or dismisses.
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_match_suggestions", request_id = %Uuid::new_v4()))]
    pub async fn artist_match_suggestions(
        &self,
        actor: &Actor,
        artist: &str,
    ) -> Result<ArtistMatchSuggestions> {
        let started = Instant::now();
        let result = tokio::time::timeout(REQUEST_BUDGET, self.read_suggestions(actor, artist))
            .await
            .unwrap_or(Err(Error::Conflict(
                "Artist suggestions took too long. Try again.".into(),
            )));
        tracing::debug!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "success"),
            suggestions = result.as_ref().map_or(0, |found| found.items.len()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist match suggestions loaded"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_match_dismiss", request_id = %Uuid::new_v4()))]
    pub async fn dismiss_artist_match_suggestion(
        &self,
        actor: &Actor,
        artist: &str,
        request: DismissArtistMatchSuggestion,
    ) -> Result<ArtistMatchSuggestions> {
        let started = Instant::now();
        let result = self
            .record_dismissal(actor, artist, &request.group_id)
            .await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "dismissed"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist match suggestion dismissed"
        );
        result?;
        self.artist_match_suggestions(actor, artist).await
    }

    /// Forgets a dismissal so the suggestion is offered again. Restoring a suggestion that was
    /// never dismissed is the same as restoring it twice: nothing changes.
    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_match_restore", request_id = %Uuid::new_v4()))]
    pub async fn restore_artist_match_suggestion(
        &self,
        actor: &Actor,
        artist: &str,
        request: RestoreArtistMatchSuggestion,
    ) -> Result<ArtistMatchSuggestions> {
        let started = Instant::now();
        let result = self
            .remove_dismissal(actor, artist, &request.group_id)
            .await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "restored"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist match suggestion restored"
        );
        result?;
        self.artist_match_suggestions(actor, artist).await
    }

    /// Opens a write transaction after checking the session, that the credit is the account's own
    /// and that the chosen artist belongs to the account.
    async fn guarded_transaction(
        &self,
        actor: &Actor,
        artist: &str,
        group: &str,
    ) -> Result<Transaction<'static, Sqlite>> {
        if artist.len() > MAX_ID_LENGTH || group.is_empty() || group.len() > MAX_ID_LENGTH {
            return Err(Error::NotFound);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        credit_name(&mut tx, actor, artist).await?;
        let known: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM account_artist_identity_groups WHERE user_id=? AND id=?)",
        )
        .bind(actor.id())
        .bind(group)
        .fetch_one(&mut *tx)
        .await?;
        if !known {
            return Err(Error::NotFound);
        }
        Ok(tx)
    }

    async fn remove_dismissal(&self, actor: &Actor, artist: &str, group: &str) -> Result<()> {
        let mut tx = self.guarded_transaction(actor, artist, group).await?;
        sqlx::query(
            "DELETE FROM artist_match_dismissals WHERE user_id=? AND artist_id=? AND group_id=?",
        )
        .bind(actor.id())
        .bind(artist)
        .bind(group)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn record_dismissal(&self, actor: &Actor, artist: &str, group: &str) -> Result<()> {
        let mut tx = self.guarded_transaction(actor, artist, group).await?;
        sqlx::query(
            "INSERT OR IGNORE INTO artist_match_dismissals(user_id,artist_id,group_id) VALUES(?,?,?)",
        )
        .bind(actor.id())
        .bind(artist)
        .bind(group)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn read_suggestions(
        &self,
        actor: &Actor,
        artist: &str,
    ) -> Result<ArtistMatchSuggestions> {
        if artist.len() > MAX_ID_LENGTH {
            return Err(Error::NotFound);
        }
        let name = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            credit_name(&mut connection, actor, artist).await?
        };
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut candidates = QueryBuilder::<Sqlite>::new("SELECT DISTINCT choice.group_id ");
        candidates.push(current_choices());
        candidates
            .push_bind(actor.id().to_owned())
            .push(" AND lower(trim(artist.name))=")
            .push_bind(name.trim().to_lowercase())
            .push(" ORDER BY choice.group_id LIMIT ")
            .push_bind(MAX_GROUPS);
        let found: Vec<String> = candidates.build_query_scalar().fetch_all(&mut *tx).await?;
        let dismissed: HashSet<String> = sqlx::query_scalar::<_, String>(
            "SELECT group_id FROM artist_match_dismissals WHERE user_id=? AND artist_id=?",
        )
        .bind(actor.id())
        .bind(artist)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        let base = undecided_tracks(&mut tx, actor, artist).await?;
        let mut items = Vec::new();
        let mut dismissed_items = Vec::new();
        for id in found {
            let facts = facts(&mut tx, actor, &base, &id).await?;
            if facts.undecided_tracks == 0 {
                continue;
            }
            let Some(group) = group_summary(&mut tx, actor, &id).await? else {
                continue;
            };
            let item = ArtistMatchSuggestion {
                group,
                shared_albums: facts.shared_albums,
                same_length_tracks: facts.same_length_tracks,
                undecided_tracks: facts.undecided_tracks,
            };
            if dismissed.contains(&id) {
                dismissed_items.push(item);
            } else {
                items.push(item);
            }
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        items.sort_by(ranking);
        dismissed_items.sort_by(ranking);
        Ok(ArtistMatchSuggestions {
            artist_id: artist.into(),
            items,
            dismissed: dismissed_items.len() as i64,
            dismissed_items,
        })
    }
}

fn ranking(left: &ArtistMatchSuggestion, right: &ArtistMatchSuggestion) -> std::cmp::Ordering {
    let score = |item: &ArtistMatchSuggestion| {
        item.shared_albums.saturating_mul(10) + item.same_length_tracks
    };
    score(right)
        .cmp(&score(left))
        .then_with(|| left.group.id.cmp(&right.group.id))
}

async fn credit_name(
    connection: &mut SqliteConnection,
    actor: &Actor,
    artist: &str,
) -> Result<String> {
    sqlx::query_scalar(
        "SELECT a.name FROM account_artists a
         JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
         WHERE a.user_id=? AND a.id=? LIMIT 1",
    )
    .bind(actor.id())
    .bind(artist)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)
}

/// The credit's tracks with no current artist choice, read once for every candidate group.
async fn undecided_tracks(
    connection: &mut SqliteConnection,
    actor: &Actor,
    artist: &str,
) -> Result<String> {
    let user = actor.id().to_owned();
    let mut query: QueryBuilder<'static, Sqlite> = QueryBuilder::new(
        "WITH credit_rec AS MATERIALIZED (
            SELECT rc.recording_id AS id FROM account_recording_credits rc
            JOIN recordings r ON r.id=rc.recording_id
            JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
            WHERE rc.user_id=",
    );
    query
        .push_bind(user.clone())
        .push(" AND rc.artist_id=")
        .push_bind(artist.to_owned())
        .push(" ORDER BY rc.recording_id LIMIT ")
        .push_bind(MAX_CREDIT_RECORDINGS);
    query.push("), decided AS MATERIALIZED (SELECT choice.recording_id AS id ");
    query.push(current_choices());
    query.push_bind(user);
    query.push(
        " AND choice.recording_id IN (SELECT id FROM credit_rec))
        SELECT json_group_array(id) FROM credit_rec WHERE id NOT IN (SELECT id FROM decided)",
    );
    Ok(query.build_query_scalar().fetch_one(connection).await?)
}

async fn facts(
    connection: &mut SqliteConnection,
    actor: &Actor,
    undecided: &str,
    group: &str,
) -> Result<Facts> {
    let user = actor.id().to_owned();
    let mut query: QueryBuilder<'static, Sqlite> =
        QueryBuilder::new("WITH member AS MATERIALIZED (SELECT choice.recording_id AS id ");
    query.push(current_choices());
    query
        .push_bind(user.clone())
        .push(" AND choice.group_id=")
        .push_bind(group.to_owned())
        .push(" LIMIT ")
        .push_bind(MAX_GROUP_RECORDINGS);
    query.push("), undecided AS (SELECT item.value AS id FROM json_each(");
    query.push_bind(undecided.to_owned());
    query.push(") item WHERE NOT EXISTS (SELECT 1 FROM recording_artist_associations assoc
                JOIN account_artist_identity_groups kept ON kept.user_id=assoc.user_id AND kept.id=");
    query.push_bind(group.to_owned());
    query.push(
        " AND kept.provider_key=assoc.provider_key AND kept.provider_artist_id=assoc.provider_artist_id
                WHERE assoc.user_id=",
    );
    query.push_bind(user.clone());
    query.push(format!(
        " AND assoc.recording_id=item.value AND NOT ({LINKED}))),
        scope AS (SELECT id FROM undecided UNION SELECT id FROM member),
        appear AS (
            SELECT source.recording_id,meta.release_id FROM sources source
            JOIN source_metadata meta ON meta.source_id=source.id
            WHERE source.recording_id IN (SELECT id FROM scope) AND meta.release_id IS NOT NULL
            UNION
            SELECT p.recording_id,p.release_id FROM provider_release_appearances p
            WHERE p.recording_id IN (SELECT id FROM scope)),
        titles AS (
            SELECT DISTINCT appear.recording_id,lower(trim(release.title)) AS title
            FROM appear JOIN account_releases release ON release.id=appear.release_id AND release.user_id="
    ));
    query.push_bind(user);
    query.push(")
        SELECT
            (SELECT count(DISTINCT t.title) FROM titles t WHERE t.recording_id IN (SELECT id FROM undecided)
                AND t.title IN (SELECT m.title FROM titles m WHERE m.recording_id IN (SELECT id FROM member))) AS shared_albums,
            (SELECT count(*) FROM undecided c JOIN recordings cr ON cr.id=c.id WHERE EXISTS (
                SELECT 1 FROM member mm JOIN recordings mr ON mr.id=mm.id
                WHERE lower(trim(mr.title))=lower(trim(cr.title)) AND abs(mr.duration_ms-cr.duration_ms)<=");
    query.push_bind(LENGTH_TOLERANCE_MS);
    query.push(
        ")) AS same_length_tracks,
            (SELECT count(*) FROM undecided) AS undecided_tracks",
    );
    Ok(query.build_query_as().fetch_one(connection).await?)
}
