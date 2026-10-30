use super::{album_editions::readable_association, authority::provider_columns, policy};
use crate::{Actor, ArtistCatalog, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_RELEASE_GROUPS: usize = 50;
const MAX_IDENTITY_BYTES: usize = 256;
const ALBUMS_PER_GROUP: usize = 3;
const CANDIDATES_PER_GROUP: i64 = 8;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadDiscographyLibrary {
    pub release_group_ids: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LibraryAlbum {
    pub release_id: String,
    pub edition_id: String,
    pub track_count: i64,
    pub playable_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LibraryReleaseGroup {
    pub release_group_id: String,
    pub albums: Vec<LibraryAlbum>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DiscographyLibrary {
    pub association_id: String,
    pub groups: Vec<LibraryReleaseGroup>,
}

type Choice = (String, String, String, String, String);

impl ArtistCatalog {
    /// Which of the account's albums are linked to the given provider release groups.
    /// A link is the edition the account chose for an album, so no name is ever compared.
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "discography_library", request_id = %Uuid::new_v4()))]
    pub async fn discography_library(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadDiscographyLibrary,
    ) -> Result<DiscographyLibrary> {
        let started = Instant::now();
        let ids = request.release_group_ids;
        let distinct: HashSet<&String> = ids.iter().collect();
        if ids.is_empty()
            || ids.len() > MAX_RELEASE_GROUPS
            || distinct.len() != ids.len()
            || ids
                .iter()
                .any(|id| id.is_empty() || id.len() > MAX_IDENTITY_BYTES)
        {
            return Err(Error::Invalid(
                "Use between one and 50 distinct bounded release group identities".into(),
            ));
        }
        let requested = serde_json::to_string(&ids)
            .map_err(|_| Error::Internal("Release groups could not be encoded".into()))?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let settings = policy::read(&mut tx, actor).await?;
        let reviewed = readable_association(&mut tx, actor, &settings, recording, association)
            .await?
            .ok_or(Error::NotFound)?;
        let (kind, installation) = provider_columns(&reviewed.provider);
        let candidates: Vec<Choice> = sqlx::query_as(
            "SELECT release_group_id, release_id, edition_id, recording_id, association_id FROM (
                SELECT c.release_group_id, c.release_id, c.edition_id, c.recording_id, c.association_id,
                    row_number() OVER (PARTITION BY c.release_group_id ORDER BY c.updated_at DESC, c.release_id) AS position
                FROM album_edition_choices c
                WHERE c.user_id = ? AND c.provider_kind = ? AND c.plugin_installation_id IS ?
                    AND c.release_group_id IN (SELECT value FROM json_each(?))
            ) WHERE position <= ? ORDER BY release_group_id, position",
        )
        .bind(actor.id())
        .bind(kind)
        .bind(installation)
        .bind(&requested)
        .bind(CANDIDATES_PER_GROUP)
        .fetch_all(&mut *tx)
        .await?;
        let mut readable: HashMap<(String, String), bool> = HashMap::new();
        let mut usable = Vec::new();
        for choice in candidates {
            let key = (choice.3.clone(), choice.4.clone());
            let allowed = match readable.get(&key) {
                Some(allowed) => *allowed,
                None => {
                    let allowed = readable_association(&mut tx, actor, &settings, &key.0, &key.1)
                        .await?
                        .is_some();
                    readable.insert(key, allowed);
                    allowed
                }
            };
            if allowed {
                usable.push(choice);
            }
        }
        let wanted = serde_json::to_string(
            &usable
                .iter()
                .map(|choice| [&choice.1, &choice.3])
                .collect::<Vec<_>>(),
        )
        .map_err(|_| Error::Internal("Library albums could not be encoded".into()))?;
        let counts: HashMap<String, (i64, i64)> = if usable.is_empty() {
            HashMap::new()
        } else {
            self.catalog
                .release_counts_on(&mut tx, actor, &wanted)
                .await?
                .into_iter()
                .filter(|counts| counts.contains_wanted)
                .map(|counts| {
                    (
                        counts.release_id,
                        (counts.track_count, counts.playable_count),
                    )
                })
                .collect()
        };
        tx.commit().await?;
        let mut groups: Vec<LibraryReleaseGroup> = ids
            .into_iter()
            .map(|release_group_id| LibraryReleaseGroup {
                release_group_id,
                albums: Vec::new(),
            })
            .collect();
        for (group, release_id, edition_id, ..) in usable {
            let Some((track_count, playable_count)) = counts.get(&release_id).copied() else {
                continue;
            };
            if let Some(found) = groups
                .iter_mut()
                .find(|candidate| candidate.release_group_id == group)
                .filter(|found| found.albums.len() < ALBUMS_PER_GROUP)
            {
                found.albums.push(LibraryAlbum {
                    release_id,
                    edition_id,
                    track_count,
                    playable_count,
                });
            }
        }
        tracing::debug!(
            requested = groups.len(),
            linked = groups
                .iter()
                .filter(|group| !group.albums.is_empty())
                .count(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Discography library availability read completed"
        );
        Ok(DiscographyLibrary {
            association_id: reviewed.id,
            groups,
        })
    }
}

#[cfg(test)]
#[path = "tests/library_availability_fixture.rs"]
mod fixture;
#[cfg(test)]
#[path = "tests/library_availability_scope.rs"]
mod scope;
#[cfg(test)]
#[path = "tests/library_availability_work.rs"]
mod work;
