use super::{Catalog, cover_art::Fetch};
use crate::{Actor, Error, Result, plugins::Operation};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest, Capability};
use sqlx::{FromRow, SqliteConnection};

const MAX_IDENTIFIER_BYTES: usize = 64;

const PROVIDER_CREDIT: &str = "FROM account_artists a
    CROSS JOIN account_recording_credits rc ON rc.user_id = a.user_id AND rc.artist_id = a.id
    CROSS JOIN recordings r ON r.id = rc.recording_id AND r.library_id = a.library_id
    CROSS JOIN libraries l ON l.id = r.library_id AND l.owner_id = a.user_id
    CROSS JOIN metadata_observations o ON o.id = r.metadata_observation_id AND o.recording_id = r.id
        AND o.origin = 'provider_response'
    CROSS JOIN provider_items binding ON binding.id = r.metadata_binding_id AND binding.id = o.provider_binding_id
        AND binding.recording_id = r.id AND binding.retired = 0
        AND binding.installation_id = o.plugin_installation_id AND binding.provider_recording_id = o.provider_recording_id
    CROSS JOIN plugin_installations p ON p.id = binding.installation_id AND p.user_id = a.user_id AND p.active = 1
        AND json_extract(p.manifest,'$.id') = o.provider
    CROSS JOIN plugin_services service ON service.id = p.service_id AND service.active = 1
    CROSS JOIN recording_credited_artist_observations credit ON credit.observation_id = o.id AND credit.recording_id = r.id
        AND credit.role = 'main' AND credit.provider_artist_id IS NOT NULL AND credit.name = a.name COLLATE BINARY
    WHERE EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork')
        AND NOT EXISTS(SELECT 1 FROM recording_artist_identity_choices choice
            JOIN account_artist_identity_groups musicbrainz ON musicbrainz.user_id = choice.user_id
                AND musicbrainz.id = choice.group_id AND musicbrainz.provider_kind = 'builtin_musicbrainz'
            WHERE choice.user_id = a.user_id AND choice.artist_id = a.id AND choice.retired = 0
                AND choice.association_id IS NOT NULL
                AND NOT EXISTS(SELECT 1 FROM cover_art_lookups photo WHERE photo.user_id = choice.user_id
                    AND photo.kind = 'artist' AND photo.mbid = musicbrainz.provider_artist_id AND photo.state = 'missing'))";

pub(crate) fn exists_for(outer: &str) -> String {
    format!(
        "EXISTS(SELECT 1 {PROVIDER_CREDIT} AND a.user_id = {outer}.user_id AND a.id = {outer}.id)"
    )
}

pub(crate) async fn photos_possible(
    connection: &mut SqliteConnection,
    account: &str,
) -> Result<bool> {
    if !super::availability::automatic_artwork_allowed(connection, account).await? {
        return Ok(false);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM plugin_installations p WHERE p.user_id = ? AND p.active = 1
            AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork'))",
    )
    .bind(account)
    .fetch_one(connection)
    .await?)
}

pub(super) fn valid_identifier(id: &str) -> bool {
    (1..=MAX_IDENTIFIER_BYTES).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[derive(FromRow, PartialEq, Eq)]
pub(super) struct Target {
    account_id: String,
    artist_id: String,
    library_id: String,
    recording_id: String,
    observation_id: String,
    binding_id: String,
    binding_revision: i64,
    credit_position: i64,
    artist_operation_id: Option<String>,
    installation_id: String,
    installation_revision: i64,
    provider_artist_id: String,
}

pub(super) struct PhotoAuthority {
    operation: Operation,
    target: Target,
}

impl PhotoAuthority {
    pub(super) async fn current_on(
        &self,
        connection: &mut SqliteConnection,
        account: &str,
        artist: &str,
    ) -> Result<bool> {
        if let Err(error) = self.operation.recheck(connection).await {
            return match error {
                Error::Conflict(_) | Error::NotFound => Ok(false),
                error => Err(error),
            };
        }
        Ok(target(connection, account, artist).await?.as_ref() == Some(&self.target))
    }
}

pub(super) async fn target(
    connection: &mut SqliteConnection,
    account: &str,
    artist: &str,
) -> Result<Option<Target>> {
    Ok(sqlx::query_as(&format!(
        "SELECT a.user_id AS account_id,a.id AS artist_id,r.library_id,r.id AS recording_id,
            o.id AS observation_id,binding.id AS binding_id,binding.revision AS binding_revision,
            credit.position AS credit_position,
            (SELECT personal.artist_operation_id FROM personal_recording_metadata personal
             WHERE personal.user_id = a.user_id AND personal.recording_id = r.id) AS artist_operation_id,
            p.id AS installation_id,p.revision AS installation_revision,
            credit.provider_artist_id AS provider_artist_id
         {PROVIDER_CREDIT} AND a.user_id = ? AND a.id = ? ORDER BY r.id,credit.position LIMIT 1"
    ))
    .bind(account)
    .bind(artist)
    .fetch_optional(connection)
    .await?)
}

impl Catalog {
    pub(super) async fn credited_artist_photo(&self, account: &str, artist: &str) -> Result<Fetch> {
        let mut connection = self.store.pool.acquire().await?;
        let actor = Actor::unattended_on(&mut connection, account).await?;
        let target = target(&mut connection, account, artist).await?;
        drop(connection);
        let Some(target) = target else {
            return Ok(Fetch::Absent);
        };
        let operation = self
            .plugins
            .operation(
                &actor,
                &target.installation_id,
                target.installation_revision,
                &[Capability::Artwork],
            )
            .await?;
        let request = ArtworkRequest {
            entity: ArtworkEntity::Artist,
            provider_id: target.provider_artist_id.clone(),
        };
        match operation.artwork(&request).await {
            Ok(Some(photo)) => Ok(Fetch::Found(
                photo,
                Some(Box::new(PhotoAuthority { operation, target })),
            )),
            Ok(None) => Ok(Fetch::Deferred),
            Err(Error::NotFound | Error::Invalid(_)) => Ok(Fetch::Absent),
            Err(error) => Err(error),
        }
    }
}
