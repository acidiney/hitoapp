use super::{
    AlbumEditionTrackListAvailability, MetadataProvider, ReadAlbumEditionTrackList,
    ReadEditionTrackList, authority::provider_columns, editions::EditionRead,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::CatalogEditionTrackList;
use hito_plugin_sdk::{CatalogCreditMember, CatalogTrackKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddAlbum {
    pub request_id: Uuid,
    pub recording_id: String,
    pub association_id: String,
    pub release_group_id: String,
    pub edition_id: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddArtistAlbum {
    pub request_id: Uuid,
    pub release_group_id: String,
    pub edition_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct AlbumAddition {
    pub id: Uuid,
    pub release_id: String,
    pub added: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct AlbumAdditionUndone {
    pub removed: u32,
    pub kept: u32,
}

struct Missing {
    occurrence: String,
    title: String,
    artist: String,
    duration_ms: i64,
    disc: i64,
    track: i64,
}

fn credit(members: &[CatalogCreditMember]) -> String {
    members
        .iter()
        .map(|member| format!("{}{}", member.credited_name, member.join_phrase))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn digest(release: &str, request: &AddAlbum) -> String {
    digest_text(&format!(
        "album-addition\n{release}\n{}\n{}\n{}\n{}",
        request.recording_id, request.association_id, request.release_group_id, request.edition_id
    ))
}

fn digest_text(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn replay(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    request_id: &Uuid,
    digest: &str,
) -> Result<Option<AlbumAddition>> {
    let found: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id,request_digest,release_id,added FROM album_additions WHERE user_id = ?1 AND request_id = ?2",
    )
    .bind(actor.id())
    .bind(request_id.to_string())
    .fetch_optional(connection)
    .await?;
    let Some((id, stored, release_id, added)) = found else {
        return Ok(None);
    };
    if stored != digest {
        return Err(Error::Conflict(
            "This request identity was already used to add a different album".into(),
        ));
    }
    Ok(Some(AlbumAddition {
        id: Uuid::parse_str(&id).map_err(|_| Error::Internal("Stored addition id".into()))?,
        release_id,
        added: added as u32,
    }))
}

impl ArtistCatalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn add_album(
        &self,
        actor: &Actor,
        release: &str,
        request: AddAlbum,
    ) -> Result<AlbumAddition> {
        actor.require_unlocked()?;
        if request.request_id.is_nil() {
            return Err(Error::Invalid(
                "Use a nonempty album addition request identity".into(),
            ));
        }
        let started = Instant::now();
        let digest = digest(release, &request);
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            if let Some(done) = replay(&mut connection, actor, &request.request_id, &digest).await?
            {
                return Ok(done);
            }
        }
        let (read, provider) = self
            .read_album_edition_state(
                actor,
                release,
                ReadAlbumEditionTrackList {
                    recording_id: request.recording_id.clone(),
                    association_id: request.association_id.clone(),
                    release_group_id: request.release_group_id.clone(),
                    edition_id: request.edition_id.clone(),
                },
            )
            .await?;
        let (
            AlbumEditionTrackListAvailability::Available {
                edition,
                matches,
                attached_occurrences,
                ..
            },
            Some(provider),
        ) = (read, provider)
        else {
            return Err(Error::Upstream(
                "The album's track list could not be read. Try again.",
            ));
        };
        let owned: HashSet<String> = matches
            .into_iter()
            .map(|matched| matched.occurrence_id)
            .chain(attached_occurrences)
            .collect();
        self.add_edition(
            actor,
            &request.request_id,
            &digest,
            provider,
            *edition,
            owned,
            started,
        )
        .await
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn add_artist_album(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: AddArtistAlbum,
    ) -> Result<AlbumAddition> {
        actor.require_unlocked()?;
        if request.request_id.is_nil() {
            return Err(Error::Invalid(
                "Use a nonempty album addition request identity".into(),
            ));
        }
        let started = Instant::now();
        let digest = digest_text(&format!(
            "artist-album-addition\n{recording}\n{association}\n{}\n{}",
            request.release_group_id, request.edition_id
        ));
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            if let Some(done) = replay(&mut connection, actor, &request.request_id, &digest).await?
            {
                return Ok(done);
            }
        }
        let EditionRead::Available(found) = self
            .read_edition(
                actor,
                recording,
                association,
                ReadEditionTrackList {
                    release_group_id: request.release_group_id.clone(),
                    edition_id: request.edition_id.clone(),
                },
            )
            .await?
        else {
            return Err(Error::Upstream(
                "The album's track list could not be read. Try again.",
            ));
        };
        let provider = found.association.provider;
        let mut owned: HashSet<String> = self
            .linked_catalog_tracks(actor, &provider, &found.edition.id, None)
            .await?
            .into_iter()
            .map(|linked| linked.occurrence_id)
            .collect();
        if let MetadataProvider::HttpPlugin { installation_id } = &provider {
            let (linked, bound) = self
                .linked_recordings(actor, installation_id, &found.edition)
                .await?;
            for track in found.edition.media.iter().flat_map(|medium| &medium.tracks) {
                if track.recording_id.as_ref().is_some_and(|provider_id| {
                    linked.contains_key(provider_id) || bound.contains(provider_id)
                }) {
                    owned.insert(track.id.clone());
                }
            }
        }
        self.add_edition(
            actor,
            &request.request_id,
            &digest,
            provider,
            found.edition,
            owned,
            started,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn add_edition(
        &self,
        actor: &Actor,
        request_id: &Uuid,
        digest: &str,
        provider: MetadataProvider,
        edition: CatalogEditionTrackList,
        owned: HashSet<String>,
        started: Instant,
    ) -> Result<AlbumAddition> {
        let album_artist = edition
            .artist_credit
            .as_deref()
            .map(credit)
            .filter(|artist| !artist.is_empty());
        let missing: Vec<Missing> = edition
            .media
            .iter()
            .flat_map(|medium| {
                medium
                    .tracks
                    .iter()
                    .map(move |track| (medium.position, track))
            })
            .filter(|(_, track)| {
                track.kind == CatalogTrackKind::Audio && !owned.contains(&track.id)
            })
            .map(|(disc, track)| Missing {
                occurrence: track.id.clone(),
                title: track.title.clone(),
                artist: track
                    .artist_credit
                    .as_deref()
                    .map(credit)
                    .filter(|artist| !artist.is_empty())
                    .or_else(|| album_artist.clone())
                    .unwrap_or_else(|| "Unknown artist".into()),
                duration_ms: track.duration_ms.unwrap_or(0).min(i64::MAX as u64) as i64,
                disc: i64::from(disc),
                track: i64::from(track.position),
            })
            .collect();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(done) = replay(&mut tx, actor, request_id, digest).await? {
            return Ok(done);
        }
        let library = crate::identity::personal_library(&mut tx, actor.id()).await?;
        let (kind, installation) = provider_columns(&provider);
        let identity = serde_json::json!(["catalog", kind, installation.unwrap_or(""), edition.id])
            .to_string();
        sqlx::query(
            "INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(library_id,identity_key) DO NOTHING",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&library)
        .bind(&edition.title)
        .bind(&album_artist)
        .bind(&identity)
        .execute(&mut *tx)
        .await?;
        let release_id: String = sqlx::query_scalar(
            "SELECT id FROM releases WHERE library_id = ?1 AND identity_key = ?2",
        )
        .bind(&library)
        .bind(&identity)
        .fetch_one(&mut *tx)
        .await?;
        let addition = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO album_additions(id,user_id,request_id,request_digest,release_id,added)
             VALUES(?1,?2,?3,?4,?5,?6)",
        )
        .bind(addition.to_string())
        .bind(actor.id())
        .bind(request_id.to_string())
        .bind(digest)
        .bind(&release_id)
        .bind(missing.len() as i64)
        .execute(&mut *tx)
        .await?;
        for track in &missing {
            let recording = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO recordings(id,library_id,release_id,title,artist,duration_ms,provenance)
                 VALUES(?1,?2,?3,?4,?5,?6,'catalog')",
            )
            .bind(&recording)
            .bind(&library)
            .bind(&release_id)
            .bind(&track.title)
            .bind(&track.artist)
            .bind(track.duration_ms)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO catalog_appearances(id,addition_id,recording_id,release_id,title,artist,duration_ms,disc_number,track_number)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(addition.to_string())
            .bind(&recording)
            .bind(&release_id)
            .bind(&track.title)
            .bind(&track.artist)
            .bind(track.duration_ms)
            .bind(track.disc)
            .bind(track.track)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO catalog_track_links(user_id,provider_kind,plugin_installation_id,edition_id,occurrence_id,recording_id)
                 VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(user_id,provider_kind,COALESCE(plugin_installation_id,''),edition_id,occurrence_id)
                 DO UPDATE SET recording_id = excluded.recording_id,updated_at = unixepoch()",
            )
            .bind(actor.id())
            .bind(kind)
            .bind(installation)
            .bind(&edition.id)
            .bind(&track.occurrence)
            .bind(&recording)
            .execute(&mut *tx)
            .await?;
            crate::metadata::refresh_recording_credits(&mut tx, &recording).await?;
        }
        crate::metadata::refresh_release(&mut tx, actor.id(), &release_id).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        tracing::info!(
            event = "album_addition_saved",
            added = missing.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Album tracks added to the library"
        );
        Ok(AlbumAddition {
            id: addition,
            release_id,
            added: missing.len() as u32,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), addition_id = %addition))]
    pub async fn undo_album_addition(
        &self,
        actor: &Actor,
        addition: Uuid,
    ) -> Result<AlbumAdditionUndone> {
        actor.require_unlocked()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let found: Option<(String, Option<i64>)> = sqlx::query_as(
            "SELECT release_id,undone_at FROM album_additions WHERE id = ?1 AND user_id = ?2",
        )
        .bind(addition.to_string())
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?;
        let Some((release_id, undone)) = found else {
            return Err(Error::NotFound);
        };
        if undone.is_some() {
            return Ok(AlbumAdditionUndone {
                removed: 0,
                kept: 0,
            });
        }
        let recordings: Vec<String> = sqlx::query_scalar(
            "SELECT recording_id FROM catalog_appearances WHERE addition_id = ?1 ORDER BY recording_id",
        )
        .bind(addition.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut removable = Vec::new();
        let mut kept = 0u32;
        for recording in recordings {
            if crate::library_storage::recording_is_referenced(&mut tx, &recording).await? {
                kept += 1;
            } else {
                removable.push(recording);
            }
        }
        for recording in &removable {
            sqlx::query("DELETE FROM catalog_appearances WHERE recording_id = ?1")
                .bind(recording)
                .execute(&mut *tx)
                .await?;
        }
        crate::library_storage::remove_recordings(&mut tx, &removable).await?;
        sqlx::query("UPDATE album_additions SET undone_at = unixepoch() WHERE id = ?1")
            .bind(addition.to_string())
            .execute(&mut *tx)
            .await?;
        crate::metadata::refresh_release(&mut tx, actor.id(), &release_id).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        let removed = removable.len() as u32;
        tracing::info!(
            event = "album_addition_undone",
            removed,
            kept,
            "Album addition undone"
        );
        Ok(AlbumAdditionUndone { removed, kept })
    }
}
