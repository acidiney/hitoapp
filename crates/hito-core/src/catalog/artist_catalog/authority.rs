use super::{
    ArtistAssociation, ArtistReviewContext, ArtistReviewMetadataBinding, MetadataProvider, policy,
};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;

#[derive(PartialEq, Eq, sqlx::FromRow)]
pub(super) struct Credit {
    pub library_id: String,
    pub access_grant: String,
    pub artist_id: String,
    pub credited_name: String,
    pub source_id: Option<String>,
    pub observation_id: String,
    pub credit_operation: Option<String>,
    pub metadata_binding_id: Option<String>,
    pub metadata_binding_revision: Option<i64>,
}

impl Credit {
    pub(super) async fn capture(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<Self> {
        actor.check_session_on(connection).await?;
        sqlx::query_as("SELECT r.library_id,access.id AS access_grant,credit.artist_id,artist.name AS credited_name,
            observation.source_id,observation.id AS observation_id,personal.artist_operation_id AS credit_operation,
            CASE WHEN observation.source_id IS NULL THEN primary_binding.id END AS metadata_binding_id,
            CASE WHEN observation.source_id IS NULL THEN primary_binding.revision END AS metadata_binding_revision
            FROM recordings r
            JOIN libraries library ON library.id=r.library_id
            JOIN library_access_grants access ON access.library_id=r.library_id AND access.user_id=?
            JOIN account_recording_credits credit ON credit.recording_id=r.id AND credit.user_id=access.user_id
            JOIN account_artists artist ON artist.user_id=credit.user_id AND artist.id=credit.artist_id AND artist.library_id=r.library_id
            JOIN metadata_observations observation ON observation.id=r.metadata_observation_id
                AND observation.recording_id=r.id
            LEFT JOIN sources source ON source.id=observation.source_id AND source.recording_id=r.id
            LEFT JOIN source_metadata source_state ON source_state.source_id=source.id AND source_state.observation_id=observation.id
            LEFT JOIN provider_items primary_binding ON primary_binding.id=r.metadata_binding_id
                AND primary_binding.id=observation.provider_binding_id AND primary_binding.recording_id=r.id
                AND primary_binding.installation_id=observation.plugin_installation_id
                AND primary_binding.provider_recording_id=observation.provider_recording_id
                AND primary_binding.role='primary' AND primary_binding.retired=0
            LEFT JOIN plugin_installations origin ON origin.id=primary_binding.installation_id
                AND origin.user_id=access.user_id
            LEFT JOIN personal_recording_metadata personal ON personal.recording_id=r.id AND personal.user_id=access.user_id
            WHERE r.id=? AND (
                (observation.source_id IS NOT NULL AND source.id IS NOT NULL AND source_state.observation_id=observation.id)
                OR (observation.source_id IS NULL AND observation.origin='provider_response'
                    AND primary_binding.id IS NOT NULL AND origin.id IS NOT NULL AND library.owner_id=access.user_id)
            )")
            .bind(actor.id()).bind(recording).fetch_optional(connection).await?.ok_or(Error::NotFound)
    }

    pub(super) fn matches(&self, association: &ArtistAssociation) -> bool {
        self.library_id == association.library_id
            && self.artist_id == association.artist_id
            && self.source_id == association.source_id
            && self.observation_id == association.observation_id
    }

    pub(super) fn matches_review(&self, context: &ArtistReviewContext) -> bool {
        self.library_id == context.library_id
            && self.artist_id == context.artist_id
            && self.credited_name == context.credited_name
            && self.source_id == context.source_id
            && self.observation_id == context.observation_id
            && self.access_grant == context.access_grant_id
            && self.credit_operation == context.artist_operation_id
            && self.metadata_binding_id.as_deref()
                == context
                    .metadata_binding
                    .as_ref()
                    .map(|binding| binding.id.as_str())
            && self.metadata_binding_revision
                == context
                    .metadata_binding
                    .as_ref()
                    .map(|binding| binding.revision)
    }

    pub(super) fn review_binding(&self) -> Result<Option<ArtistReviewMetadataBinding>> {
        match (&self.metadata_binding_id, self.metadata_binding_revision) {
            (None, None) => Ok(None),
            (Some(id), Some(revision)) => Ok(Some(ArtistReviewMetadataBinding {
                id: id.clone(),
                revision,
            })),
            _ => Err(Error::Internal(
                "The recording metadata binding is invalid".into(),
            )),
        }
    }

    pub(super) async fn recheck(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        association: &ArtistAssociation,
        policy_revision: i64,
    ) -> Result<()> {
        let current = Self::capture(connection, actor, &association.recording_id).await?;
        if current != *self || !current.matches(association) {
            return Err(changed());
        }
        let policy = policy::read(connection, actor).await?;
        if policy.revision != policy_revision || !policy::selected(&policy, &association.provider) {
            return Err(policy::changed());
        }
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recording_artist_associations
            WHERE id=? AND user_id=? AND recording_id=? AND revision=?)",
        )
        .bind(&association.id)
        .bind(actor.id())
        .bind(&association.recording_id)
        .bind(association.revision)
        .fetch_one(connection)
        .await?;
        if !retained {
            return Err(changed());
        }
        Ok(())
    }
}

pub(super) fn provider_columns(provider: &MetadataProvider) -> (&'static str, Option<&str>) {
    match provider {
        MetadataProvider::BuiltinMusicBrainz => ("builtin_musicbrainz", None),
        MetadataProvider::HttpPlugin { installation_id } => ("http_plugin", Some(installation_id)),
    }
}

pub(super) fn changed() -> Error {
    Error::Conflict("The reviewed recording credit changed. Review it again.".into())
}
