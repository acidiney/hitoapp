use super::{
    ArtistAssociation, LINKED, MetadataProvider, MetadataSettings, REQUEST_BUDGET,
    ReadEditionTrackList, associations, authority::Credit, editions::EditionRead, policy, provider,
};
use crate::{
    Actor, AlbumCatalogAnchor, ArtistCatalog, Browse, Error, Recording, Release, ReleaseDetail,
    ReleaseTrack, Result,
    catalog::{recordings_on, releases::selected},
};
use hito_plugin_sdk::CatalogEditionTrackList;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    time::Instant,
};
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_ANCHOR_CANDIDATES: i64 = 16;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadAlbumEditionTrackList {
    pub recording_id: String,
    pub association_id: String,
    pub release_group_id: String,
    pub edition_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct CatalogTrackMatch {
    pub occurrence_id: String,
    pub recording: Recording,
}

#[derive(Serialize, ToSchema)]
pub struct CatalogAdoption {
    pub installation_id: String,
    pub installation_revision: i64,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AlbumEditionTrackListAvailability {
    Available {
        association_id: String,
        edition: Box<CatalogEditionTrackList>,
        matches: Vec<CatalogTrackMatch>,
        library_only: Vec<ReleaseTrack>,
        attached_occurrences: Vec<String>,
        #[schema(required = true)]
        adoption: Option<CatalogAdoption>,
        is_album_release: bool,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
    NeedsChoice,
}

impl AlbumEditionTrackListAvailability {
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::NeedsChoice => "needs_choice",
        }
    }
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "album_edition_track_list", request_id = %Uuid::new_v4()))]
    pub async fn album_edition_track_list_until(
        &self,
        actor: &Actor,
        release: &str,
        request: ReadAlbumEditionTrackList,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AlbumEditionTrackListAvailability> {
        self.bounded_album_read(cancelled, self.read_album_edition(actor, release, request))
            .await
    }

    pub(super) async fn bounded_album_read(
        &self,
        cancelled: impl Future<Output = ()> + Send,
        read: impl Future<Output = Result<AlbumEditionTrackListAvailability>>,
    ) -> Result<AlbumEditionTrackListAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(AlbumEditionTrackListAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, read) => {
                result.unwrap_or(Ok(AlbumEditionTrackListAvailability::Unavailable))
            }
        };
        let (matched, library_only) = match &result {
            Ok(AlbumEditionTrackListAvailability::Available {
                matches,
                library_only,
                ..
            }) => (matches.len(), library_only.len()),
            _ => (0, 0),
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(AlbumEditionTrackListAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            matched,
            library_only,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Album edition track list read completed"
        );
        result
    }

    pub(super) async fn read_album_edition(
        &self,
        actor: &Actor,
        release: &str,
        request: ReadAlbumEditionTrackList,
    ) -> Result<AlbumEditionTrackListAvailability> {
        let (detail, read) = self
            .read_album_edition_tracks(actor, release, request)
            .await?;
        let (association, edition, playback_revision) = match read {
            EditionRead::Available(found) => {
                (found.association, found.edition, found.playback_revision)
            }
            EditionRead::Missing(status) => return Ok(status.into()),
        };
        let remembered = self
            .linked_catalog_tracks(actor, &association.provider, &edition.id, None)
            .await?;
        let installation = match &association.provider {
            MetadataProvider::HttpPlugin { installation_id } => Some(installation_id.as_str()),
            MetadataProvider::BuiltinMusicBrainz => None,
        };
        let (linked, bound) = match installation {
            Some(installation) => {
                self.linked_recordings(actor, installation, &edition)
                    .await?
            }
            None => Default::default(),
        };
        let mut matches = Vec::new();
        for medium in &edition.media {
            for track in &medium.tracks {
                if let Some(recording) = track
                    .recording_id
                    .as_ref()
                    .and_then(|provider_id| linked.get(provider_id))
                {
                    matches.push(CatalogTrackMatch {
                        occurrence_id: track.id.clone(),
                        recording: recording.clone(),
                    });
                }
            }
        }
        for found in remembered {
            if !matches
                .iter()
                .any(|matched| matched.occurrence_id == found.occurrence_id)
            {
                matches.push(found);
            }
        }
        let attached_occurrences = edition
            .media
            .iter()
            .flat_map(|medium| &medium.tracks)
            .filter(|track| {
                track.recording_id.as_ref().is_some_and(|provider_id| {
                    bound.contains(provider_id) && !linked.contains_key(provider_id)
                }) && !matches
                    .iter()
                    .any(|matched| matched.occurrence_id == track.id)
            })
            .map(|track| track.id.clone())
            .collect();
        let linked_ids: HashSet<&str> = matches
            .iter()
            .map(|linked| linked.recording.id.as_str())
            .collect();
        let library_only = detail
            .tracks
            .items
            .into_iter()
            .filter(|track| !linked_ids.contains(track.recording.id.as_str()))
            .collect();
        let is_album_release = match installation {
            Some(installation) => {
                self.release_is_provider_edition(actor, release, installation, &edition.id)
                    .await?
            }
            None => false,
        };
        let adoption = installation
            .zip(playback_revision)
            .map(|(installation, revision)| CatalogAdoption {
                installation_id: installation.into(),
                installation_revision: revision,
            });
        Ok(AlbumEditionTrackListAvailability::Available {
            association_id: association.id,
            edition: Box::new(edition),
            matches,
            library_only,
            attached_occurrences,
            adoption,
            is_album_release,
        })
    }

    pub(super) async fn read_album_edition_tracks(
        &self,
        actor: &Actor,
        release: &str,
        request: ReadAlbumEditionTrackList,
    ) -> Result<(ReleaseDetail, EditionRead)> {
        let detail = self.catalog.release_for_comparison(actor, release).await?;
        if !detail
            .tracks
            .items
            .iter()
            .any(|track| track.recording.id == request.recording_id)
        {
            return Err(Error::NotFound);
        }
        let read = self
            .read_edition(
                actor,
                &request.recording_id,
                &request.association_id,
                ReadEditionTrackList {
                    release_group_id: request.release_group_id,
                    edition_id: request.edition_id,
                },
            )
            .await?;
        Ok((detail, read))
    }

    async fn linked_recordings(
        &self,
        actor: &Actor,
        installation: &str,
        edition: &CatalogEditionTrackList,
    ) -> Result<(HashMap<String, Recording>, HashSet<String>)> {
        let wanted: Vec<&str> = edition
            .media
            .iter()
            .flat_map(|medium| &medium.tracks)
            .filter_map(|track| track.recording_id.as_deref())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if wanted.is_empty() {
            return Ok(Default::default());
        }
        let wanted = serde_json::to_string(&wanted)
            .map_err(|_| Error::Internal("Catalog identities could not be encoded".into()))?;
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT item.provider_recording_id, item.recording_id FROM json_each(?) wanted
            JOIN provider_items item ON item.installation_id = ? AND item.provider_recording_id = wanted.value
                AND item.role = 'primary' AND item.retired = 0
            JOIN plugin_installations installation ON installation.id = item.installation_id AND installation.user_id = ?
            JOIN recordings r ON r.id = item.recording_id AND r.metadata_binding_id = item.id
            JOIN library_members member ON member.library_id = r.library_id AND member.user_id = ?",
        )
        .bind(&wanted)
        .bind(installation)
        .bind(actor.id())
        .bind(actor.id())
        .fetch_all(&mut *connection)
        .await?;
        let bound: HashSet<String> = sqlx::query_scalar(
            "SELECT DISTINCT item.provider_recording_id FROM json_each(?) wanted
            JOIN provider_items item ON item.installation_id = ? AND item.provider_recording_id = wanted.value
                AND item.retired = 0
            JOIN plugin_installations installation ON installation.id = item.installation_id AND installation.user_id = ?",
        )
        .bind(&wanted)
        .bind(installation)
        .bind(actor.id())
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect();
        let ids = rows
            .iter()
            .map(|(_, recording)| Uuid::parse_str(recording))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Internal("A stored recording identity is invalid".into()))?;
        let recordings = recordings_on(&mut connection, &self.host, actor, &ids).await?;
        Ok((
            rows.into_iter()
                .zip(recordings)
                .map(|((provider_id, _), recording)| (provider_id, recording))
                .collect(),
            bound,
        ))
    }

    async fn release_is_provider_edition(
        &self,
        actor: &Actor,
        release: &str,
        installation: &str,
        edition: &str,
    ) -> Result<bool> {
        let key: Option<String> = sqlx::query_scalar(
            "SELECT release.identity_key FROM releases release JOIN account_releases account ON account.id = release.id
            WHERE release.id = ? AND account.user_id = ?",
        )
        .bind(release)
        .bind(actor.id())
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(key
            .and_then(|key| serde_json::from_str::<(String, String, String)>(&key).ok())
            .is_some_and(|(kind, owner, id)| {
                kind == "plugin" && owner == installation && id == edition
            }))
    }
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn release(&self, actor: &Actor, id: &str, browse: Browse) -> Result<ReleaseDetail> {
        let mut detail = self.catalog.release(actor, id, browse).await?;
        if detail.tracks.offset == 0 {
            detail.saved_edition = Some(self.saved_album_edition(actor, id).await?);
            detail.catalog_anchor = self.album_anchor(actor, &detail.release, false).await?;
        }
        Ok(detail)
    }

    pub(super) async fn album_anchor(
        &self,
        actor: &Actor,
        release: &Release,
        builtin_only: bool,
    ) -> Result<Option<AlbumCatalogAnchor>> {
        let mut candidates = selected(actor, &Browse::default(), Some(&release.id))?;
        candidates
            .push(
                "SELECT assoc.recording_id, assoc.id AS association_id FROM entries m
                JOIN recording_artist_associations assoc ON assoc.recording_id = m.recording_id AND assoc.user_id = ",
            )
            .push_bind(actor.id().to_owned())
            .push(" AND ")
            .push(LINKED)
            .push(" ORDER BY (assoc.artist_id IS ")
            .push_bind(release.artist_id.clone())
            .push(") DESC, COALESCE(m.disc_number, 1), m.track_number IS NULL, m.track_number, assoc.created_at, assoc.id LIMIT ")
            .push_bind(MAX_ANCHOR_CANDIDATES);
        let candidates = candidates
            .build_query_as()
            .fetch_all(&self.store.pool)
            .await?;
        self.first_readable_anchor(actor, candidates, builtin_only)
            .await
    }

    async fn first_readable_anchor(
        &self,
        actor: &Actor,
        candidates: Vec<AlbumCatalogAnchor>,
        builtin_only: bool,
    ) -> Result<Option<AlbumCatalogAnchor>> {
        if candidates.is_empty() {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let settings = policy::read(&mut tx, actor).await?;
        for candidate in candidates {
            if readable_association(
                &mut tx,
                actor,
                &settings,
                &candidate.recording_id,
                &candidate.association_id,
            )
            .await?
            .is_some_and(|found| {
                !builtin_only || found.provider == MetadataProvider::BuiltinMusicBrainz
            }) {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
}

pub(super) async fn readable_association(
    connection: &mut SqliteConnection,
    actor: &Actor,
    settings: &MetadataSettings,
    recording: &str,
    association: &str,
) -> Result<Option<ArtistAssociation>> {
    if !settings.enabled {
        return Ok(None);
    }
    let association = match associations::load(connection, actor, recording, association).await {
        Ok(association) => association,
        Err(Error::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !policy::selected(settings, &association.provider) {
        return Ok(None);
    }
    if let MetadataProvider::HttpPlugin { installation_id } = &association.provider
        && provider::catalog_revision(connection, actor, installation_id)
            .await?
            .is_none()
    {
        return Ok(None);
    }
    match Credit::capture(connection, actor, recording).await {
        Ok(credit) if credit.matches(connection, actor, &association).await? => {
            Ok(Some(association))
        }
        Ok(_) | Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}
