use super::{
    Artwork, Catalog, assets, availability, capacity, credited_artist,
    lookups::{self, LookupState, RETRY_AFTER_SECONDS},
    rendition,
};
use crate::{Actor, Error, Result, plugins::ProviderArtwork};
use hito_metadata_musicbrainz::CoverKind;
use sqlx::{FromRow, SqliteConnection};
use std::time::Instant;
use uuid::Uuid;

const MAX_PENDING_PER_ACCOUNT: i64 = 64;
const MAX_PENDING_ARTISTS_PER_ACCOUNT: i64 = 32;
const MAX_ATTEMPTS: i64 = 4;
const LEASE_SECONDS: i64 = 60;
const BUSY_RETRY_SECONDS: i64 = 5;

pub(super) enum Fetch {
    Found(
        ProviderArtwork,
        Option<Box<credited_artist::PhotoAuthority>>,
    ),
    Absent,
    Deferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverArtKind {
    Release,
    ReleaseGroup,
    Artist,
    CreditedArtist,
}

impl CoverArtKind {
    pub fn from_path(segment: &str) -> Option<Self> {
        match segment {
            "release" => Some(Self::Release),
            "release-group" => Some(Self::ReleaseGroup),
            "artist" => Some(Self::Artist),
            "credited-artist" => Some(Self::CreditedArtist),
            _ => None,
        }
    }

    pub(super) fn is_artist_photo(self) -> bool {
        matches!(self, Self::Artist | Self::CreditedArtist)
    }

    pub(super) fn stored(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::ReleaseGroup => "release_group",
            Self::Artist => "artist",
            Self::CreditedArtist => "credited_artist",
        }
    }

    pub(super) fn from_stored(value: &str) -> Result<Self> {
        match value {
            "release" => Ok(Self::Release),
            "release_group" => Ok(Self::ReleaseGroup),
            "artist" => Ok(Self::Artist),
            "credited_artist" => Ok(Self::CreditedArtist),
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
pub(super) struct Lookup {
    pub(super) user_id: String,
    pub(super) kind: String,
    pub(super) mbid: String,
    attempts: i64,
    pub(super) lease_until: i64,
}

fn identifier(kind: CoverArtKind, id: &str) -> Result<()> {
    if kind == CoverArtKind::CreditedArtist {
        return if credited_artist::valid_identifier(id) {
            Ok(())
        } else {
            Err(Error::Invalid("Use an artist identifier".into()))
        };
    }
    match Uuid::parse_str(id) {
        Ok(parsed) if parsed.hyphenated().to_string() == id => Ok(()),
        _ => Err(Error::Invalid("Use a MusicBrainz identifier".into())),
    }
}

pub(crate) async fn enqueue(
    connection: &mut SqliteConnection,
    account: &str,
    kind: CoverArtKind,
    mbid: &str,
) -> Result<()> {
    identifier(kind, mbid)?;
    if !super::availability::automatic_artwork_allowed(connection, account).await? {
        return Ok(());
    }
    if kind == CoverArtKind::CreditedArtist
        && credited_artist::target(connection, account, mbid)
            .await?
            .is_none()
    {
        return Ok(());
    }
    let wanted = lookups::read(connection, account, kind, mbid)
        .await?
        .is_none_or(|lookup| lookup.state.is_answered_absent() && lookup.aged);
    if !wanted {
        return Ok(());
    }
    capacity::admit_image(connection, account).await?;
    if pending_lookups(connection, account).await? >= MAX_PENDING_PER_ACCOUNT {
        return Ok(());
    }
    if kind.is_artist_photo() {
        let pending_artists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM cover_art_lookups WHERE user_id = ? AND kind = ? AND state IN ('queued','running')",
        )
        .bind(account)
        .bind(kind.stored())
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

const OBSERVED_COVER_BATCH: i64 = 32;

pub(crate) async fn queue_observed_release_covers(
    connection: &mut SqliteConnection,
    account: &str,
) -> Result<()> {
    if !super::availability::automatic_artwork_allowed(connection, account).await? {
        return Ok(());
    }
    let room = MAX_PENDING_PER_ACCOUNT - pending_lookups(connection, account).await?;
    if room <= 0 {
        return Ok(());
    }
    let candidates: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT o.entity,o.mbid
         FROM recording_musicbrainz_observations o
         JOIN recordings r ON r.id = o.recording_id AND r.metadata_observation_id = o.observation_id
         JOIN releases a ON a.id = o.release_id
         JOIN library_members m ON m.library_id = a.library_id AND m.user_id = ?1
         WHERE o.entity IN ('release','release_group')
            AND NOT EXISTS (SELECT 1 FROM release_artwork ra WHERE ra.user_id = ?1 AND ra.release_id = a.id)
            AND NOT EXISTS (SELECT 1 FROM source_metadata sm JOIN sources s ON s.id = sm.source_id
                WHERE sm.release_id = a.id AND s.provider = 'local')
            AND NOT EXISTS (SELECT 1 FROM cover_art_lookups c
                WHERE c.user_id = ?1 AND c.kind = o.entity AND c.mbid = o.mbid)
         LIMIT ?2",
    )
    .bind(account)
    .bind(room.min(OBSERVED_COVER_BATCH))
    .fetch_all(&mut *connection)
    .await?;
    for (entity, mbid) in candidates {
        match enqueue(
            connection,
            account,
            CoverArtKind::from_stored(&entity)?,
            &mbid,
        )
        .await
        {
            Ok(()) | Err(Error::Invalid(_) | Error::ArtworkStorageFull) => {}
            Err(error) => return Err(error),
        }
    }
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
        identifier(kind, mbid)?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let found = lookups::lookup_status(&mut tx, actor.id(), kind, mbid).await?;
        if matches!(found, CoverArtLookup::Unknown)
            && !kind.is_artist_photo()
            && availability::automatic_artwork_allowed(&mut tx, actor.id()).await?
        {
            capacity::admit_image(&mut tx, actor.id()).await?;
        }
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        if matches!(found, CoverArtLookup::Unknown) && kind.is_artist_photo() {
            return self.queue_artist_photo(actor, kind, mbid).await;
        }
        Ok(found)
    }

    async fn queue_artist_photo(
        &self,
        actor: &Actor,
        kind: CoverArtKind,
        mbid: &str,
    ) -> Result<CoverArtLookup> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        enqueue(&mut tx, actor.id(), kind, mbid).await?;
        let queued = lookups::read(&mut tx, actor.id(), kind, mbid).await?;
        tx.commit().await?;
        Ok(match queued {
            Some(lookup) if lookup.state.is_pending() => CoverArtLookup::Pending,
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
            match lookups::read(&mut tx, actor.id(), kind, &mbid).await? {
                Some(lookup) => {
                    if let Some(hash) = lookup.hash {
                        found = Some(assets::ready_asset(&mut tx, actor.id(), &hash).await?);
                        break;
                    }
                    if lookup.state.is_answered_absent() && lookup.aged {
                        wanted.push((kind, mbid));
                    }
                }
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

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(lookup_attempt_id))]
    pub async fn run_next_cover_art_lookup_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let Some(lookup) = self.claim_cover_lookup().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let attempt_id = Uuid::new_v4();
        tracing::Span::current().record("lookup_attempt_id", tracing::field::display(attempt_id));
        let kind = CoverArtKind::from_stored(&lookup.kind)?;
        let admission = self.admit_cover_lookup(&lookup).await?;
        if admission != "admitted" {
            tracing::info!(
                event = "cover_art_lookup",
                lookup_attempt_id = %attempt_id,
                account_id = %lookup.user_id,
                outcome = admission,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Cover art lookup finished"
            );
            return Ok(true);
        }
        let fetched = tokio::select! {
            biased;
            _ = shutdown => {
                self.finish_cover_lookup(&lookup, LookupState::Queued, None, 0, 0).await?;
                tracing::info!(event = "cover_art_interrupted", lookup_attempt_id = %attempt_id,
                    elapsed_ms = started.elapsed().as_millis() as u64, "Cover art lookup interrupted");
                return Ok(true);
            }
            fetched = self.fetch_cover(kind, &lookup) => fetched,
        };
        let outcome = match fetched {
            Ok(Fetch::Found(remote, authority)) => {
                match tokio::task::spawn_blocking(move || rendition::decode(remote))
                    .await
                    .map_err(|_| Error::Internal("Cover art validation stopped".into()))?
                {
                    Ok(image) => {
                        self.publish_cover(&lookup, image, authority.as_deref())
                            .await?
                    }
                    Err(_) => {
                        self.finish_cover_lookup(
                            &lookup,
                            LookupState::Missing,
                            None,
                            lookup.attempts,
                            0,
                        )
                        .await?;
                        "invalid"
                    }
                }
            }
            Ok(Fetch::Absent) => {
                self.finish_cover_lookup(&lookup, LookupState::Missing, None, 0, 0)
                    .await?;
                "absent"
            }
            Ok(Fetch::Deferred) => {
                self.finish_cover_lookup(
                    &lookup,
                    LookupState::Queued,
                    None,
                    lookup.attempts,
                    BUSY_RETRY_SECONDS,
                )
                .await?;
                "deferred"
            }
            Err(_) => {
                let attempts = lookup.attempts + 1;
                if attempts >= MAX_ATTEMPTS {
                    self.finish_cover_lookup(&lookup, LookupState::Unavailable, None, attempts, 0)
                        .await?;
                } else {
                    let delay = 30i64 << attempts;
                    self.finish_cover_lookup(&lookup, LookupState::Queued, None, attempts, delay)
                        .await?;
                }
                tracing::debug!(outcome = "provider_unavailable", "Cover art lookup failed");
                "failed"
            }
        };
        tracing::info!(
            event = "cover_art_lookup",
            lookup_attempt_id = %attempt_id,
            account_id = %lookup.user_id,
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Cover art lookup finished"
        );
        Ok(true)
    }

    async fn fetch_cover(
        &self,
        kind: CoverArtKind,
        lookup: &Lookup,
    ) -> std::result::Result<Fetch, String> {
        let mbid = &lookup.mbid;
        let cover = match kind {
            CoverArtKind::Release => self.cover_art.front(CoverKind::Release, mbid).await,
            CoverArtKind::ReleaseGroup => self.cover_art.front(CoverKind::ReleaseGroup, mbid).await,
            CoverArtKind::Artist => self.artist_image.photo(mbid).await,
            CoverArtKind::CreditedArtist => {
                return self
                    .credited_artist_photo(&lookup.user_id, mbid)
                    .await
                    .map_err(|error| error.to_string());
            }
        };
        cover
            .map(|cover| {
                cover.map_or(Fetch::Absent, |cover| {
                    Fetch::Found(
                        ProviderArtwork {
                            content_type: cover.content_type,
                            bytes: cover.bytes,
                        },
                        None,
                    )
                })
            })
            .map_err(|error| error.to_string())
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
             RETURNING user_id,kind,mbid,attempts,lease_until",
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
        state: LookupState,
        hash: Option<&str>,
        attempts: i64,
        delay: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE cover_art_lookups SET state = ?,content_hash = ?,attempts = ?,
                next_attempt_at = unixepoch() + ?,lease_until = NULL,updated_at = unixepoch()
             WHERE user_id = ? AND kind = ? AND mbid = ? AND state = 'running'
                AND content_hash IS NULL AND lease_until = ? AND lease_until > unixepoch()",
        )
        .bind(state.stored())
        .bind(hash)
        .bind(attempts)
        .bind(delay)
        .bind(&lookup.user_id)
        .bind(&lookup.kind)
        .bind(&lookup.mbid)
        .bind(lookup.lease_until)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    async fn admit_cover_lookup(&self, lookup: &Lookup) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let eligible = current_lookup(&mut tx, lookup).await?
            && (lookup.kind != CoverArtKind::CreditedArtist.stored()
                || credited_artist::target(&mut tx, &lookup.user_id, &lookup.mbid)
                    .await?
                    .is_some());
        let outcome = if !eligible {
            "authority_changed"
        } else {
            match capacity::admit_image(&mut tx, &lookup.user_id).await {
                Ok(()) => "admitted",
                Err(Error::ArtworkStorageFull) => "storage_full",
                Err(error) => return Err(error),
            }
        };
        if outcome != "admitted" {
            forget_lookup(&mut tx, lookup).await?;
        }
        tx.commit().await?;
        Ok(outcome)
    }

    async fn publish_cover(
        &self,
        lookup: &Lookup,
        image: rendition::ValidatedArtwork,
        authority: Option<&credited_artist::PhotoAuthority>,
    ) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = if !current_lookup(&mut tx, lookup).await? {
            false
        } else {
            match (CoverArtKind::from_stored(&lookup.kind)?, authority) {
                (CoverArtKind::CreditedArtist, Some(authority)) => {
                    authority
                        .current_on(&mut tx, &lookup.user_id, &lookup.mbid)
                        .await?
                }
                (CoverArtKind::CreditedArtist, None) | (_, Some(_)) => false,
                (_, None) => true,
            }
        };
        let outcome = if !current {
            forget_lookup(&mut tx, lookup).await?;
            "authority_changed"
        } else {
            match assets::publish_lookup(&mut tx, lookup, image).await {
                Ok(true) => "ready",
                Ok(false) => "superseded",
                Err(Error::ArtworkStorageFull) => {
                    forget_lookup(&mut tx, lookup).await?;
                    "storage_full"
                }
                Err(error) => return Err(error),
            }
        };
        tx.commit().await?;
        Ok(outcome)
    }
}

async fn pending_lookups(connection: &mut SqliteConnection, account: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM cover_art_lookups WHERE user_id = ? AND state IN ('queued','running')",
    )
    .bind(account)
    .fetch_one(connection)
    .await?)
}

async fn current_lookup(connection: &mut SqliteConnection, lookup: &Lookup) -> Result<bool> {
    let current: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cover_art_lookups lookup JOIN users u ON u.id = lookup.user_id
         WHERE lookup.user_id = ? AND lookup.kind = ? AND lookup.mbid = ?
            AND lookup.state = 'running' AND lookup.content_hash IS NULL
            AND lookup.lease_until = ? AND lookup.lease_until > unixepoch())",
    )
    .bind(&lookup.user_id)
    .bind(&lookup.kind)
    .bind(&lookup.mbid)
    .bind(lookup.lease_until)
    .fetch_one(&mut *connection)
    .await?;
    Ok(current && availability::automatic_artwork_allowed(connection, &lookup.user_id).await?)
}

async fn forget_lookup(connection: &mut SqliteConnection, lookup: &Lookup) -> Result<()> {
    sqlx::query(
        "DELETE FROM cover_art_lookups WHERE user_id = ? AND kind = ? AND mbid = ?
            AND state = 'running' AND content_hash IS NULL AND lease_until = ?",
    )
    .bind(&lookup.user_id)
    .bind(&lookup.kind)
    .bind(&lookup.mbid)
    .bind(lookup.lease_until)
    .execute(connection)
    .await?;
    Ok(())
}
