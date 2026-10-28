use super::{
    changes::{ArtistIdentityChangeKind, history_revision, items, latest},
    split::group_key,
};
use crate::{
    Actor, Catalog, Error, Result,
    catalog::artist_catalog::shared_corrections::{SharedCorrectionRef, applied_for_change},
};
use serde::Serialize;
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

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
    /// The applied shared correction made from this change; only administrators see it.
    pub shared: Option<SharedCorrectionRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ArtistIdentityGroupChanges {
    pub group_id: String,
    pub revision: i64,
    pub latest: Option<ArtistIdentityChangeSummary>,
}

pub(crate) struct SplitChange {
    pub id: String,
    pub revision: i64,
    pub kind: String,
    pub builtin: bool,
    pub provider_artist_id: String,
    pub observations: Vec<(String, String)>,
}

/// The latest change of a group with the observation each moved recording was reviewed under.
pub(crate) async fn split_change(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<SplitChange> {
    let key = group_key(connection, actor, group).await?;
    let latest = latest(connection, actor, group)
        .await?
        .ok_or_else(|| Error::Conflict("There is no artist identity change to share".into()))?;
    let observations = items(connection, &latest.id)
        .await?
        .into_iter()
        .filter_map(|item| Some((item.recording_id, item.from_observation_id?)))
        .collect();
    Ok(SplitChange {
        builtin: key.is_builtin(),
        provider_artist_id: key.provider_artist_id,
        id: latest.id,
        revision: latest.revision,
        kind: latest.kind,
        observations,
    })
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
                let admin = actor.require_admin().is_ok();
                let shared = if admin {
                    applied_for_change(&mut tx, &latest.id).await?
                } else {
                    None
                };
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
                    shareable: split && active > 0 && shared.is_none() && key.is_builtin() && admin,
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
}
