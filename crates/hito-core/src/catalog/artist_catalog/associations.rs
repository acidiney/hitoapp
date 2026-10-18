use super::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCreditRole, AssociateRecordingArtist,
    MAX_ASSOCIATIONS, MetadataProvider, REQUEST_BUDGET,
    authority::{self, Credit},
    policy, provider,
};
use crate::{Actor, Catalog, Error, Result, identity::recording_access};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct Stored {
    id: String,
    user_id: String,
    library_id: String,
    recording_id: String,
    artist_id: String,
    source_id: Option<String>,
    observation_id: String,
    provider_kind: String,
    plugin_installation_id: Option<String>,
    provider_artist_id: String,
    revision: i64,
}

impl Stored {
    fn project(self) -> Result<ArtistAssociation> {
        let provider = match (self.provider_kind.as_str(), self.plugin_installation_id) {
            ("builtin_musicbrainz", None) => MetadataProvider::BuiltinMusicBrainz,
            ("http_plugin", Some(installation_id)) => {
                MetadataProvider::HttpPlugin { installation_id }
            }
            _ => {
                return Err(Error::Internal(
                    "Stored artist association is invalid".into(),
                ));
            }
        };
        Ok(ArtistAssociation {
            id: self.id,
            account_id: self.user_id,
            library_id: self.library_id,
            recording_id: self.recording_id,
            artist_id: self.artist_id,
            source_id: self.source_id,
            observation_id: self.observation_id,
            role: ArtistCreditRole::RecordingArtist,
            provider,
            provider_artist_id: self.provider_artist_id,
            evidence: ArtistAssociationEvidence::UserConfirmed,
            revision: self.revision,
        })
    }
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_associations", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_associations(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<Vec<ArtistAssociation>> {
        let started = Instant::now();
        let result = tokio::time::timeout(REQUEST_BUDGET, async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            recording_access(&mut tx, actor, recording).await?;
            let rows: Vec<Stored> = sqlx::query_as(
                "SELECT * FROM recording_artist_associations
                WHERE user_id=? AND recording_id=? ORDER BY created_at,id LIMIT ?",
            )
            .bind(actor.id())
            .bind(recording)
            .bind(MAX_ASSOCIATIONS)
            .fetch_all(&mut *tx)
            .await?;
            let associations = rows
                .into_iter()
                .map(Stored::project)
                .collect::<Result<Vec<_>>>()?;
            tx.commit().await?;
            Ok(associations)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist associations exceeded the time limit",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            association_count = result.as_ref().map(|items| items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist association read completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_association_review", request_id = %Uuid::new_v4()))]
    pub async fn associate_recording_artist(
        &self,
        actor: &Actor,
        recording: &str,
        request: AssociateRecordingArtist,
    ) -> Result<ArtistAssociation> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.review_artist(actor, recording, request),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist association review exceeded the time limit",
        )));
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist association review completed"
        );
        result
    }

    async fn review_artist(
        &self,
        actor: &Actor,
        recording: &str,
        request: AssociateRecordingArtist,
    ) -> Result<ArtistAssociation> {
        actor.check_session(&self.store).await?;
        provider::validate_artist_id(&request.provider, &request.provider_artist_id)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let credit = Credit::capture(&mut tx, actor, recording).await?;
        if credit.artist_id != request.artist_id
            || credit.source_id != request.source_id
            || credit.observation_id != request.observation_id
        {
            return Err(authority::changed());
        }
        let policy = policy::read(&mut tx, actor).await?;
        if policy.revision != request.policy_revision
            || !policy::selected(&policy, &request.provider)
        {
            return Err(policy::changed());
        }
        policy::approved(&mut tx, actor, &request.provider).await?;
        let association = retain(&mut tx, actor, recording, &credit.library_id, &request).await?;
        tx.commit().await?;
        Ok(association)
    }
}

pub(super) async fn retain(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    library: &str,
    request: &AssociateRecordingArtist,
) -> Result<ArtistAssociation> {
    let (kind, installation) = authority::provider_columns(&request.provider);
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM recording_artist_associations
            WHERE user_id=? AND recording_id=? AND artist_id=? AND source_id IS ? AND observation_id=?
                AND provider_kind=? AND plugin_installation_id IS ? AND provider_artist_id=?",
    )
    .bind(actor.id())
    .bind(recording)
    .bind(&request.artist_id)
    .bind(&request.source_id)
    .bind(&request.observation_id)
    .bind(kind)
    .bind(installation)
    .bind(&request.provider_artist_id)
    .fetch_optional(&mut *connection)
    .await?;
    let id = if let Some(existing) = existing {
        existing
    } else {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM recording_artist_associations WHERE user_id=? AND recording_id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_one(&mut *connection)
        .await?;
        if count >= MAX_ASSOCIATIONS {
            return Err(Error::Conflict(
                "This recording already has 32 retained artist associations".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,
                source_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence)
                VALUES (?,?,?,?,?,?,?,'recording_artist',?,?,?,'user_confirmed')")
            .bind(&id).bind(actor.id()).bind(library).bind(recording).bind(&request.artist_id)
            .bind(&request.source_id).bind(&request.observation_id).bind(kind).bind(installation)
            .bind(&request.provider_artist_id).execute(&mut *connection).await?;
        id
    };
    load(connection, actor, recording, &id).await
}

pub(super) async fn load(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    association: &str,
) -> Result<ArtistAssociation> {
    let row: Stored = sqlx::query_as(
        "SELECT * FROM recording_artist_associations WHERE id=? AND user_id=? AND recording_id=?",
    )
    .bind(association)
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    row.project()
}
