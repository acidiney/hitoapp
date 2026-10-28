use super::{receipts, split::group_key};
use crate::{Actor, Catalog, Error, MetadataProvider, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

pub(super) const MAX_MOVED: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SplitDestination {
    /// Each recording returns to the library credit it was tagged with.
    Credit,
    /// Each recording moves to an exact artist identity it already reviewed.
    Identity {
        provider: MetadataProvider,
        provider_artist_id: String,
    },
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitArtistIdentityGroup {
    pub request_id: Uuid,
    pub expected_revision: i64,
    #[serde(default)]
    pub recordings: Vec<Uuid>,
    #[serde(default)]
    pub releases: Vec<Uuid>,
    pub destination: SplitDestination,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoArtistIdentityChange {
    pub request_id: Uuid,
    pub expected_revision: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtistIdentityChangeKind {
    Split,
    Undo,
}

impl ArtistIdentityChangeKind {
    pub(super) fn column(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Undo => "undo",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ArtistIdentityChangeReceipt {
    pub request_id: Uuid,
    pub group_id: String,
    pub revision: i64,
    pub kind: ArtistIdentityChangeKind,
    pub recordings: i64,
    pub destination_group_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistIdentityChangeSummary {
    pub revision: i64,
    pub kind: ArtistIdentityChangeKind,
    pub recordings: i64,
    /// Items whose recording no longer carries the credit the change was made for.
    pub retired: i64,
    pub destination_group_id: Option<String>,
    pub undoable: bool,
    /// An administrator can apply this split for every account.
    pub shareable: bool,
    /// An applied shared correction was made from this change.
    pub shared: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistIdentityGroupChanges {
    pub group_id: String,
    pub revision: i64,
    pub latest: Option<ArtistIdentityChangeSummary>,
}

/// The selected side of one recording's identity choice, without its revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Side {
    pub group: String,
    pub association: String,
    pub operation: Option<String>,
    pub observation: String,
    pub order: i64,
}

#[derive(sqlx::FromRow)]
pub(super) struct Item {
    pub recording_id: String,
    pub from_revision: i64,
    pub from_group_id: Option<String>,
    pub from_association_id: Option<String>,
    pub from_artist_id: String,
    pub from_operation_id: Option<String>,
    pub from_observation_id: Option<String>,
    pub from_order: Option<i64>,
    pub to_revision: i64,
    pub to_group_id: Option<String>,
    pub to_association_id: Option<String>,
    pub to_operation_id: Option<String>,
    pub to_observation_id: Option<String>,
    pub to_order: Option<i64>,
}

impl Item {
    fn to_side(&self) -> Option<Side> {
        side(
            &self.to_group_id,
            &self.to_association_id,
            &self.to_operation_id,
            &self.to_observation_id,
            self.to_order,
        )
    }

    /// The item that undoes this one: the same recording, with both sides exchanged.
    fn reversed(&self) -> Self {
        Self {
            recording_id: self.recording_id.clone(),
            from_revision: self.to_revision,
            from_group_id: self.to_group_id.clone(),
            from_association_id: self.to_association_id.clone(),
            from_artist_id: self.from_artist_id.clone(),
            from_operation_id: self.to_operation_id.clone(),
            from_observation_id: self.to_observation_id.clone(),
            from_order: self.to_order,
            to_revision: self.to_revision + 1,
            to_group_id: self.from_group_id.clone(),
            to_association_id: self.from_association_id.clone(),
            to_operation_id: self.from_operation_id.clone(),
            to_observation_id: self.from_observation_id.clone(),
            to_order: self.from_order,
        }
    }
}

fn side(
    group: &Option<String>,
    association: &Option<String>,
    operation: &Option<String>,
    observation: &Option<String>,
    order: Option<i64>,
) -> Option<Side> {
    Some(Side {
        group: group.clone()?,
        association: association.clone()?,
        operation: operation.clone(),
        observation: observation.clone()?,
        order: order?,
    })
}

pub(super) struct Latest {
    pub id: String,
    pub revision: i64,
    pub kind: String,
    pub destination_group_id: Option<String>,
}

pub(super) async fn history_revision(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(max(revision),0) FROM artist_identity_changes WHERE user_id=? AND group_id=?",
    )
    .bind(actor.id())
    .bind(group)
    .fetch_one(connection)
    .await?)
}

async fn latest(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<Option<Latest>> {
    let row: Option<(String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id,revision,kind,destination_group_id FROM artist_identity_changes
         WHERE user_id=? AND group_id=? ORDER BY revision DESC LIMIT 1",
    )
    .bind(actor.id())
    .bind(group)
    .fetch_optional(connection)
    .await?;
    Ok(
        row.map(|(id, revision, kind, destination_group_id)| Latest {
            id,
            revision,
            kind,
            destination_group_id,
        }),
    )
}

pub(super) async fn items(connection: &mut SqliteConnection, change: &str) -> Result<Vec<Item>> {
    Ok(sqlx::query_as(
        "SELECT recording_id,from_revision,from_group_id,from_association_id,from_artist_id,from_operation_id,
            from_observation_id,from_order,to_revision,to_group_id,to_association_id,to_operation_id,
            to_observation_id,to_order FROM artist_identity_change_items
         WHERE change_id=? AND retired_reason IS NULL ORDER BY recording_id",
    )
    .bind(change)
    .fetch_all(connection)
    .await?)
}

pub(super) async fn write_choice(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    revision: i64,
    artist: &str,
    side: Option<&Side>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,
            artist_id,artist_operation_id,observation_id,selection_order)
         VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(user_id,recording_id) DO UPDATE SET revision=excluded.revision,
            association_id=excluded.association_id,group_id=excluded.group_id,artist_id=excluded.artist_id,
            artist_operation_id=excluded.artist_operation_id,observation_id=excluded.observation_id,retired=0,
            selected_at=unixepoch(),selection_order=excluded.selection_order",
    )
    .bind(actor.id())
    .bind(recording)
    .bind(revision)
    .bind(side.map(|side| &side.association))
    .bind(side.map(|side| &side.group))
    .bind(side.map(|_| artist))
    .bind(side.and_then(|side| side.operation.as_ref()))
    .bind(side.map(|side| &side.observation))
    .bind(side.map(|side| side.order))
    .execute(connection)
    .await?;
    Ok(())
}

pub(super) async fn insert_change(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
    revision: i64,
    kind: ArtistIdentityChangeKind,
    destination: Option<&str>,
    order: i64,
    items: &[Item],
) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,destination_group_id,receipt_order)
         VALUES(?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(actor.id())
    .bind(group)
    .bind(revision)
    .bind(kind.column())
    .bind(destination)
    .bind(order)
    .execute(&mut *connection)
    .await?;
    for item in items {
        sqlx::query(
            "INSERT INTO artist_identity_change_items(change_id,recording_id,from_revision,from_group_id,
                from_association_id,from_artist_id,from_operation_id,from_observation_id,from_order,to_revision,
                to_group_id,to_association_id,to_operation_id,to_observation_id,to_order)
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(&item.recording_id)
        .bind(item.from_revision)
        .bind(&item.from_group_id)
        .bind(&item.from_association_id)
        .bind(&item.from_artist_id)
        .bind(&item.from_operation_id)
        .bind(&item.from_observation_id)
        .bind(item.from_order)
        .bind(item.to_revision)
        .bind(&item.to_group_id)
        .bind(&item.to_association_id)
        .bind(&item.to_operation_id)
        .bind(&item.to_observation_id)
        .bind(item.to_order)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub(super) fn stale() -> Error {
    Error::Conflict("The artist identity changes moved on. Review their current revision".into())
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_identity_changes_read", request_id = %Uuid::new_v4()))]
    pub async fn artist_identity_group_changes(
        &self,
        actor: &Actor,
        group: &str,
    ) -> Result<ArtistIdentityGroupChanges> {
        let started = Instant::now();
        let result = self.read_identity_changes(actor, group).await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity changes loaded"
        );
        result
    }

    async fn read_identity_changes(
        &self,
        actor: &Actor,
        group: &str,
    ) -> Result<ArtistIdentityGroupChanges> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let key = group_key(&mut tx, actor, group).await?;
        let revision = history_revision(&mut tx, actor, group).await?;
        let latest = match latest(&mut tx, actor, group).await? {
            Some(latest) => {
                let (active, retired): (i64, i64) = sqlx::query_as(
                    "SELECT count(*) FILTER (WHERE retired_reason IS NULL),
                        count(*) FILTER (WHERE retired_reason IS NOT NULL)
                     FROM artist_identity_change_items WHERE change_id=?",
                )
                .bind(&latest.id)
                .fetch_one(&mut *tx)
                .await?;
                let shared: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM shared_artist_corrections correction
                     WHERE correction.source_change_id=? AND
                        (SELECT state.applied FROM shared_artist_correction_states state
                         WHERE state.correction_seq=correction.seq ORDER BY state.revision DESC LIMIT 1)=1)",
                )
                .bind(&latest.id)
                .fetch_one(&mut *tx)
                .await?;
                let split = latest.kind == "split";
                Some(ArtistIdentityChangeSummary {
                    revision: latest.revision,
                    kind: if split {
                        ArtistIdentityChangeKind::Split
                    } else {
                        ArtistIdentityChangeKind::Undo
                    },
                    recordings: active,
                    retired,
                    destination_group_id: latest.destination_group_id,
                    undoable: active > 0,
                    shareable: split
                        && active > 0
                        && !shared
                        && key.is_builtin()
                        && actor.require_admin().is_ok(),
                    shared,
                })
            }
            None => None,
        };
        tx.commit().await?;
        Ok(ArtistIdentityGroupChanges {
            group_id: group.into(),
            revision,
            latest,
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_identity_undo", request_id = %request.request_id))]
    pub async fn undo_artist_identity_change(
        &self,
        actor: &Actor,
        group: &str,
        request: UndoArtistIdentityChange,
    ) -> Result<ArtistIdentityChangeReceipt> {
        let started = Instant::now();
        let result = self.accept_identity_undo(actor, group, request).await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            revision = result.as_ref().map_or(0, |receipt| receipt.revision),
            recordings = result.as_ref().map_or(0, |receipt| receipt.recordings),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity undo finished"
        );
        result
    }

    async fn accept_identity_undo(
        &self,
        actor: &Actor,
        group: &str,
        request: UndoArtistIdentityChange,
    ) -> Result<ArtistIdentityChangeReceipt> {
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request and a nonnegative revision".into(),
            ));
        }
        let digest = receipts::fingerprint("hito-artist-identity-undo:v1", group, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        group_key(&mut tx, actor, group).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        if history_revision(&mut tx, actor, group).await? != request.expected_revision {
            return Err(stale());
        }
        let latest = latest(&mut tx, actor, group)
            .await?
            .ok_or_else(|| Error::Conflict("There is no artist identity change to undo".into()))?;
        let reversed: Vec<Item> = items(&mut tx, &latest.id)
            .await?
            .iter()
            .map(Item::reversed)
            .collect();
        if reversed.is_empty() {
            return Err(Error::Conflict(
                "This artist identity change no longer applies to any recording".into(),
            ));
        }
        for item in &reversed {
            let current: Option<(i64, bool, Option<String>)> = sqlx::query_as(
                "SELECT revision,retired,association_id FROM recording_artist_identity_choices
                 WHERE user_id=? AND recording_id=?",
            )
            .bind(actor.id())
            .bind(&item.recording_id)
            .fetch_optional(&mut *tx)
            .await?;
            let unchanged = matches!(&current, Some((revision, false, association))
                if *revision == item.from_revision && *association == item.from_association_id);
            if !unchanged {
                return Err(Error::Conflict(
                    "A recording changed after this artist identity change. Review it again".into(),
                ));
            }
        }
        let revision = request.expected_revision + 1;
        let receipt = ArtistIdentityChangeReceipt {
            request_id: request.request_id,
            group_id: group.into(),
            revision,
            kind: ArtistIdentityChangeKind::Undo,
            recordings: reversed.len() as i64,
            destination_group_id: None,
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        for item in &reversed {
            write_choice(
                &mut tx,
                actor,
                &item.recording_id,
                item.to_revision,
                &item.from_artist_id,
                item.to_side().as_ref(),
            )
            .await?;
        }
        insert_change(
            &mut tx,
            actor,
            group,
            revision,
            ArtistIdentityChangeKind::Undo,
            None,
            order,
            &reversed,
        )
        .await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}
