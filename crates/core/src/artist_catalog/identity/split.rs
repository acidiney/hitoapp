use super::{
    ArtistIdentityChangeKind, ArtistIdentityChangeReceipt, SplitArtistIdentityGroup,
    SplitDestination,
    changes::{self, Item, MAX_MOVED, Side},
    groups::current_choices,
    receipts,
};
use crate::artist_catalog::{
    LINKED, associations,
    authority::{Credit, provider_columns},
    policy, provider,
};
use crate::{Actor, ArtistCatalog, Error, MetadataProvider, Result};
use sqlx::SqliteConnection;
use std::{collections::HashSet, time::Instant};
use uuid::Uuid;

pub(super) struct GroupKey {
    pub provider_kind: String,
    pub installation: Option<String>,
    pub provider_artist_id: String,
}

impl GroupKey {
    pub(super) fn is_builtin(&self) -> bool {
        self.provider_kind == "builtin_musicbrainz"
    }
}

pub(super) async fn group_key(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<GroupKey> {
    let row: Option<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT provider_kind,plugin_installation_id,provider_artist_id
         FROM account_artist_identity_groups WHERE id=? AND user_id=?",
    )
    .bind(group)
    .bind(actor.id())
    .fetch_optional(connection)
    .await?;
    let (provider_kind, installation, provider_artist_id) = row.ok_or(Error::NotFound)?;
    Ok(GroupKey {
        provider_kind,
        installation,
        provider_artist_id,
    })
}

#[derive(sqlx::FromRow)]
struct Member {
    recording_id: String,
    revision: i64,
    group_id: String,
    association_id: String,
    artist_id: String,
    artist_operation_id: Option<String>,
    observation_id: String,
    selection_order: i64,
}

async fn members(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
    request: &SplitArtistIdentityGroup,
) -> Result<Vec<Member>> {
    let recordings = serde_json::to_string(&request.recordings).unwrap_or_default();
    let releases = serde_json::to_string(&request.releases).unwrap_or_default();
    let rows = sqlx::query_as(&format!(
        "SELECT choice.recording_id,choice.revision,choice.group_id,choice.association_id,choice.artist_id,
            choice.artist_operation_id,choice.observation_id,choice.selection_order {}
         ? AND choice.group_id=? AND (choice.recording_id IN (SELECT value FROM json_each(?))
            OR choice.recording_id IN (
                SELECT source.recording_id FROM sources source JOIN source_metadata m ON m.source_id=source.id
                WHERE m.release_id IN (SELECT value FROM json_each(?))
                UNION SELECT p.recording_id FROM provider_release_appearances p
                WHERE p.release_id IN (SELECT value FROM json_each(?))))
         ORDER BY choice.recording_id LIMIT ?",
        current_choices()
    ))
    .bind(actor.id())
    .bind(group)
    .bind(&recordings)
    .bind(&releases)
    .bind(&releases)
    .bind((MAX_MOVED + 1) as i64)
    .fetch_all(connection)
    .await?;
    Ok(rows)
}

async fn destination_side(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    destination: &str,
    provider: &MetadataProvider,
    artist: &str,
    order: i64,
) -> Result<Side> {
    let credit = Credit::capture(connection, actor, recording)
        .await
        .map_err(|error| match error {
            Error::NotFound => no_review(),
            error => error,
        })?;
    let (kind, installation) = provider_columns(provider);
    let association: Option<String> = sqlx::query_scalar(&format!(
        "SELECT assoc.id FROM recording_artist_associations assoc
         WHERE assoc.user_id=? AND assoc.recording_id=? AND assoc.provider_kind=?
            AND assoc.plugin_installation_id IS ? AND assoc.provider_artist_id=?
            AND assoc.artist_id=? AND assoc.observation_id=? AND assoc.source_id IS ? AND {LINKED}"
    ))
    .bind(actor.id())
    .bind(recording)
    .bind(kind)
    .bind(installation)
    .bind(artist)
    .bind(&credit.artist_id)
    .bind(&credit.observation_id)
    .bind(&credit.source_id)
    .fetch_optional(&mut *connection)
    .await?;
    let association = association.ok_or_else(no_review)?;
    let loaded = associations::load(connection, actor, recording, &association).await?;
    if !credit.matches(&loaded) {
        return Err(no_review());
    }
    Ok(Side {
        group: destination.into(),
        association,
        operation: credit.credit_operation,
        observation: credit.observation_id,
        order,
    })
}

fn no_review() -> Error {
    Error::Conflict(
        "Every selected recording needs a current review of the destination artist".into(),
    )
}

impl ArtistCatalog {
    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_identity_split", request_id = %request.request_id))]
    pub async fn split_artist_identity_group(
        &self,
        actor: &Actor,
        group: &str,
        request: SplitArtistIdentityGroup,
    ) -> Result<ArtistIdentityChangeReceipt> {
        let started = Instant::now();
        let result = self.accept_split(actor, group, request).await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            revision = result.as_ref().map_or(0, |receipt| receipt.revision),
            recordings = result.as_ref().map_or(0, |receipt| receipt.recordings),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity split finished"
        );
        result
    }

    async fn accept_split(
        &self,
        actor: &Actor,
        group: &str,
        request: SplitArtistIdentityGroup,
    ) -> Result<ArtistIdentityChangeReceipt> {
        if request.request_id.is_nil()
            || request.expected_revision < 0
            || request.recordings.iter().any(Uuid::is_nil)
            || request.releases.iter().any(Uuid::is_nil)
            || (request.recordings.is_empty() && request.releases.is_empty())
            || request.recordings.len() > MAX_MOVED
            || request.releases.len() > MAX_MOVED
        {
            return Err(Error::Invalid(
                "Select up to 100 recordings and 100 releases, with a nonempty request".into(),
            ));
        }
        if let SplitDestination::Identity {
            provider,
            provider_artist_id,
        } = &request.destination
        {
            provider::validate_artist_id(provider, provider_artist_id)?;
        }
        let digest = receipts::fingerprint("hito-artist-identity-split:v1", group, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let source = group_key(&mut tx, actor, group).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let current = changes::history_revision(&mut tx, actor, group).await?;
        if current != request.expected_revision {
            return Err(changes::stale());
        }
        let members = members(&mut tx, actor, group, &request).await?;
        if members.is_empty() || members.len() > MAX_MOVED {
            return Err(Error::Conflict(
                "Select between 1 and 100 current recordings of this artist".into(),
            ));
        }
        let found: HashSet<&str> = members.iter().map(|m| m.recording_id.as_str()).collect();
        if request
            .recordings
            .iter()
            .any(|recording| !found.contains(recording.to_string().as_str()))
        {
            return Err(Error::Conflict(
                "Every selected recording must currently use this artist".into(),
            ));
        }
        let destination = match &request.destination {
            SplitDestination::Credit => None,
            SplitDestination::Identity {
                provider,
                provider_artist_id,
            } => {
                let (kind, installation) = provider_columns(provider);
                if kind == source.provider_kind
                    && installation == source.installation.as_deref()
                    && *provider_artist_id == source.provider_artist_id
                {
                    return Err(Error::Invalid(
                        "Choose a different artist to move recordings to".into(),
                    ));
                }
                let settings = policy::read(&mut tx, actor).await?;
                if !settings.enabled || !policy::selected(&settings, provider) {
                    return Err(policy::changed());
                }
                policy::approved(&mut tx, actor, provider).await?;
                let id: String = sqlx::query_scalar(
                    "INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,plugin_installation_id,provider_artist_id)
                     VALUES(?,?,?,?,?) ON CONFLICT(user_id,provider_key,provider_artist_id)
                     DO UPDATE SET provider_artist_id=excluded.provider_artist_id RETURNING id",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(actor.id())
                .bind(kind)
                .bind(installation)
                .bind(provider_artist_id)
                .fetch_one(&mut *tx)
                .await?;
                Some(id)
            }
        };
        let revision = current + 1;
        let receipt = ArtistIdentityChangeReceipt {
            request_id: request.request_id,
            group_id: group.into(),
            revision,
            kind: ArtistIdentityChangeKind::Split,
            recordings: members.len() as i64,
            destination_group_id: destination.clone(),
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        let mut items = Vec::with_capacity(members.len());
        for member in &members {
            let to = match (&request.destination, &destination) {
                (
                    SplitDestination::Identity {
                        provider,
                        provider_artist_id,
                    },
                    Some(destination),
                ) => Some(
                    destination_side(
                        &mut tx,
                        actor,
                        &member.recording_id,
                        destination,
                        provider,
                        provider_artist_id,
                        order,
                    )
                    .await?,
                ),
                _ => None,
            };
            let to_revision = member.revision + 1;
            changes::write_choice(
                &mut tx,
                actor,
                &member.recording_id,
                to_revision,
                &member.artist_id,
                to.as_ref(),
            )
            .await?;
            items.push(Item {
                recording_id: member.recording_id.clone(),
                from_revision: member.revision,
                from_group_id: Some(member.group_id.clone()),
                from_association_id: Some(member.association_id.clone()),
                from_artist_id: member.artist_id.clone(),
                from_operation_id: member.artist_operation_id.clone(),
                from_observation_id: Some(member.observation_id.clone()),
                from_order: Some(member.selection_order),
                to_revision,
                to_group_id: to.as_ref().map(|side| side.group.clone()),
                to_association_id: to.as_ref().map(|side| side.association.clone()),
                to_operation_id: to.as_ref().and_then(|side| side.operation.clone()),
                to_observation_id: to.as_ref().map(|side| side.observation.clone()),
                to_order: to.as_ref().map(|side| side.order),
            });
        }
        changes::insert_change(
            &mut tx,
            actor,
            changes::NewChange {
                group,
                revision,
                kind: ArtistIdentityChangeKind::Split,
                destination: destination.as_deref(),
                order,
            },
            &items,
        )
        .await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}
