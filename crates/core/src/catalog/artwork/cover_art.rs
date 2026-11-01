use super::{Artwork, Catalog, assets};
use crate::{Actor, Error, Result, plugins::ProviderArtwork};
use hito_metadata_musicbrainz::CoverKind;
use sqlx::{FromRow, SqliteConnection};
use std::time::Instant;
use uuid::Uuid;

const MAX_PENDING_PER_ACCOUNT: i64 = 64;
const MAX_PENDING_ARTISTS_PER_ACCOUNT: i64 = 32;
const MAX_ATTEMPTS: i64 = 4;
const LEASE_SECONDS: i64 = 60;
const RETRY_AFTER_SECONDS: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverArtKind {
    Release,
    ReleaseGroup,
    Artist,
}

impl CoverArtKind {
    pub fn from_path(segment: &str) -> Option<Self> {
        match segment {
            "release" => Some(Self::Release),
            "release-group" => Some(Self::ReleaseGroup),
            "artist" => Some(Self::Artist),
            _ => None,
        }
    }

    fn stored(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::ReleaseGroup => "release_group",
            Self::Artist => "artist",
        }
    }

    fn from_stored(value: &str) -> Result<Self> {
        match value {
            "release" => Ok(Self::Release),
            "release_group" => Ok(Self::ReleaseGroup),
            "artist" => Ok(Self::Artist),
            _ => Err(Error::Internal("Stored cover art kind is invalid".into())),
        }
    }
}

pub enum CoverArtLookup {
    Ready(Artwork),
    Pending,
    Missing,
    Unavailable,
    Unknown,
}

#[derive(FromRow)]
struct Lookup {
    user_id: String,
    kind: String,
    mbid: String,
    attempts: i64,
}

fn canonical(mbid: &str) -> Result<()> {
    match Uuid::parse_str(mbid) {
        Ok(id) if id.hyphenated().to_string() == mbid => Ok(()),
        _ => Err(Error::Invalid("Use a MusicBrainz identifier".into())),
    }
}

pub(crate) async fn enqueue(
    connection: &mut SqliteConnection,
    account: &str,
    kind: CoverArtKind,
    mbid: &str,
) -> Result<()> {
    canonical(mbid)?;
    if !super::availability::automatic_artwork_allowed(connection, account).await? {
        return Ok(());
    }
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cover_art_lookups WHERE user_id = ? AND state IN ('queued','running')",
    )
    .bind(account)
    .fetch_one(&mut *connection)
    .await?;
    if pending >= MAX_PENDING_PER_ACCOUNT {
        return Ok(());
    }
    if kind == CoverArtKind::Artist {
        let pending_artists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM cover_art_lookups WHERE user_id = ? AND kind = 'artist' AND state IN ('queued','running')",
        )
        .bind(account)
        .fetch_one(&mut *connection)
        .await?;
        if pending_artists >= MAX_PENDING_ARTISTS_PER_ACCOUNT {
            return Ok(());
        }
    }
    sqlx::query(
        "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES (?,?,?,'queued')
         ON CONFLICT(user_id,kind,mbid) DO UPDATE SET state = 'queued',attempts = 0,next_attempt_at = 0,
            updated_at = unixepoch()
         WHERE state IN ('unavailable','missing') AND updated_at < unixepoch() - ?",
    )
    .bind(account)
    .bind(kind.stored())
    .bind(mbid)
    .bind(RETRY_AFTER_SECONDS)
    .execute(connection)
    .await?;
    Ok(())
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn cover_art(
        &self,
        actor: &Actor,
        kind: CoverArtKind,
        mbid: &str,
    ) -> Result<CoverArtLookup> {
        canonical(mbid)?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let row: Option<(String, Option<String>, bool)> = sqlx::query_as(
            "SELECT state,content_hash,updated_at < unixepoch() - ? FROM cover_art_lookups WHERE user_id = ? AND kind = ? AND mbid = ?",
        )
        .bind(RETRY_AFTER_SECONDS)
        .bind(actor.id())
        .bind(kind.stored())
        .bind(mbid)
        .fetch_optional(&mut *tx)
        .await?;
        let found = match row {
            None => CoverArtLookup::Unknown,
            Some((state, hash, aged)) => match (state.as_str(), hash) {
                ("ready", Some(_))
                    if kind == CoverArtKind::Artist
                        && !super::availability::automatic_artwork_allowed(&mut tx, actor.id())
                            .await? =>
                {
                    CoverArtLookup::Unknown
                }
                ("ready", Some(hash)) => {
                    let asset: (String, Vec<u8>) = sqlx::query_as(
                        "SELECT content_type,bytes FROM artwork_assets WHERE user_id = ? AND content_hash = ?",
                    )
                    .bind(actor.id())
                    .bind(hash)
                    .fetch_one(&mut *tx)
                    .await?;
                    CoverArtLookup::Ready(assets::stored(asset)?)
                }
                ("missing" | "unavailable", _) if aged && kind == CoverArtKind::Artist => {
                    CoverArtLookup::Unknown
                }
                ("missing", _) => CoverArtLookup::Missing,
                ("unavailable", _) => CoverArtLookup::Unavailable,
                _ => CoverArtLookup::Pending,
            },
        };
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        if matches!(found, CoverArtLookup::Unknown) && kind == CoverArtKind::Artist {
            return self.queue_artist_photo(actor, mbid).await;
        }
        Ok(found)
    }

    async fn queue_artist_photo(&self, actor: &Actor, mbid: &str) -> Result<CoverArtLookup> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        enqueue(&mut tx, actor.id(), CoverArtKind::Artist, mbid).await?;
        let queued: Option<String> = sqlx::query_scalar(
            "SELECT state FROM cover_art_lookups WHERE user_id = ? AND kind = 'artist' AND mbid = ?",
        )
        .bind(actor.id())
        .bind(mbid)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(match queued.as_deref() {
            Some("queued" | "running") => CoverArtLookup::Pending,
            _ => CoverArtLookup::Unknown,
        })
    }

    pub(super) async fn library_cover_art(
        &self,
        actor: &Actor,
        release: &str,
    ) -> Result<Option<Artwork>> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let identifiers: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT o.entity,o.mbid FROM recording_musicbrainz_observations o
             JOIN recordings r ON r.id = o.recording_id AND r.metadata_observation_id = o.observation_id
             WHERE o.release_id = ? AND o.entity IN ('release','release_group')
             ORDER BY o.entity = 'release' DESC,o.mbid LIMIT 4",
        )
        .bind(release)
        .fetch_all(&mut *tx)
        .await?;
        let mut found = None;
        let mut wanted = Vec::new();
        for (entity, mbid) in identifiers {
            let kind = CoverArtKind::from_stored(&entity)?;
            let known: Option<(String, Option<String>, bool)> = sqlx::query_as(
                "SELECT state,content_hash,updated_at < unixepoch() - ? FROM cover_art_lookups
                 WHERE user_id = ? AND kind = ? AND mbid = ?",
            )
            .bind(RETRY_AFTER_SECONDS)
            .bind(actor.id())
            .bind(kind.stored())
            .bind(&mbid)
            .fetch_optional(&mut *tx)
            .await?;
            match known {
                Some((_, Some(hash), _)) => {
                    let asset: (String, Vec<u8>) = sqlx::query_as(
                        "SELECT content_type,bytes FROM artwork_assets WHERE user_id = ? AND content_hash = ?",
                    )
                    .bind(actor.id())
                    .bind(hash)
                    .fetch_one(&mut *tx)
                    .await?;
                    found = Some(assets::stored(asset)?);
                    break;
                }
                Some((state, None, true)) if state == "missing" || state == "unavailable" => {
                    wanted.push((kind, mbid));
                }
                Some(_) => {}
                None => wanted.push((kind, mbid)),
            }
        }
        tx.commit().await?;
        if found.is_none() && !wanted.is_empty() {
            let mut write = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            for (kind, mbid) in wanted {
                enqueue(&mut write, actor.id(), kind, &mbid).await?;
            }
            write.commit().await?;
        }
        actor.check_session(&self.store).await?;
        Ok(found)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_cover_art_lookup_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let Some(lookup) = self.claim_cover_lookup().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let kind = CoverArtKind::from_stored(&lookup.kind)?;
        let allowed: Option<bool> =
            sqlx::query_scalar("SELECT artwork FROM account_metadata_automation WHERE user_id = ?")
                .bind(&lookup.user_id)
                .fetch_optional(&self.store.pool)
                .await?;
        if !allowed.unwrap_or(true) {
            self.forget_cover_lookup(&lookup).await?;
            tracing::info!(
                event = "cover_art_dropped",
                "Cover art lookup dropped: setting is off"
            );
            return Ok(true);
        }
        let fetched = tokio::select! {
            biased;
            _ = shutdown => {
                self.finish_cover_lookup(&lookup, "queued", None, 0, 0).await?;
                tracing::info!(event = "cover_art_interrupted", "Cover art lookup interrupted");
                return Ok(true);
            }
            fetched = self.fetch_cover(kind, &lookup.mbid) => fetched,
        };
        let outcome = match fetched {
            Ok(Some(cover)) => {
                let remote = ProviderArtwork {
                    content_type: cover.content_type,
                    bytes: cover.bytes,
                };
                match tokio::task::spawn_blocking(move || assets::decode(remote))
                    .await
                    .map_err(|_| Error::Internal("Cover art validation stopped".into()))?
                {
                    Ok(image) => {
                        self.publish_cover(&lookup, image).await?;
                        "ready"
                    }
                    Err(_) => {
                        self.finish_cover_lookup(&lookup, "missing", None, lookup.attempts, 0)
                            .await?;
                        "invalid"
                    }
                }
            }
            Ok(None) => {
                self.finish_cover_lookup(&lookup, "missing", None, 0, 0)
                    .await?;
                "absent"
            }
            Err(error) => {
                let attempts = lookup.attempts + 1;
                if attempts >= MAX_ATTEMPTS {
                    self.finish_cover_lookup(&lookup, "unavailable", None, attempts, 0)
                        .await?;
                } else {
                    let delay = 30i64 << attempts;
                    self.finish_cover_lookup(&lookup, "queued", None, attempts, delay)
                        .await?;
                }
                tracing::debug!(error = %error, "Cover art lookup failed");
                "failed"
            }
        };
        tracing::info!(
            event = "cover_art_lookup",
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Cover art lookup finished"
        );
        Ok(true)
    }

    async fn fetch_cover(
        &self,
        kind: CoverArtKind,
        mbid: &str,
    ) -> std::result::Result<
        Option<hito_metadata_musicbrainz::CoverArt>,
        hito_metadata_musicbrainz::Error,
    > {
        match kind {
            CoverArtKind::Release => self.cover_art.front(CoverKind::Release, mbid).await,
            CoverArtKind::ReleaseGroup => self.cover_art.front(CoverKind::ReleaseGroup, mbid).await,
            CoverArtKind::Artist => self.artist_image.photo(mbid).await,
        }
    }

    async fn claim_cover_lookup(&self) -> Result<Option<Lookup>> {
        let due: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM cover_art_lookups
             WHERE (state = 'queued' AND next_attempt_at <= unixepoch())
                OR (state = 'running' AND lease_until <= unixepoch()))",
        )
        .fetch_one(&self.store.pool)
        .await?;
        if !due {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let lookup = sqlx::query_as(
            "UPDATE cover_art_lookups SET state = 'running',lease_until = unixepoch() + ?
             WHERE rowid = (SELECT rowid FROM cover_art_lookups
                WHERE (state = 'queued' AND next_attempt_at <= unixepoch())
                   OR (state = 'running' AND lease_until <= unixepoch())
                ORDER BY next_attempt_at,updated_at LIMIT 1)
             RETURNING user_id,kind,mbid,attempts",
        )
        .bind(LEASE_SECONDS)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(lookup)
    }

    async fn finish_cover_lookup(
        &self,
        lookup: &Lookup,
        state: &str,
        hash: Option<&str>,
        attempts: i64,
        delay: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE cover_art_lookups SET state = ?,content_hash = ?,attempts = ?,
                next_attempt_at = unixepoch() + ?,lease_until = NULL,updated_at = unixepoch()
             WHERE user_id = ? AND kind = ? AND mbid = ? AND state = 'running'",
        )
        .bind(state)
        .bind(hash)
        .bind(attempts)
        .bind(delay)
        .bind(&lookup.user_id)
        .bind(&lookup.kind)
        .bind(&lookup.mbid)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    async fn forget_cover_lookup(&self, lookup: &Lookup) -> Result<()> {
        sqlx::query("DELETE FROM cover_art_lookups WHERE user_id = ? AND kind = ? AND mbid = ?")
            .bind(&lookup.user_id)
            .bind(&lookup.kind)
            .bind(&lookup.mbid)
            .execute(&self.store.pool)
            .await?;
        Ok(())
    }

    async fn publish_cover(&self, lookup: &Lookup, image: assets::ValidatedArtwork) -> Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let hash = assets::store(&mut tx, &lookup.user_id, image).await?;
        sqlx::query(
            "UPDATE cover_art_lookups SET state = 'ready',content_hash = ?,attempts = 0,
                lease_until = NULL,updated_at = unixepoch()
             WHERE user_id = ? AND kind = ? AND mbid = ? AND state = 'running'",
        )
        .bind(&hash)
        .bind(&lookup.user_id)
        .bind(&lookup.kind)
        .bind(&lookup.mbid)
        .execute(&mut *tx)
        .await?;
        assets::prune(&mut tx, Some(&lookup.user_id)).await?;
        tx.commit().await?;
        Ok(())
    }
}
