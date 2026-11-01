use super::super::recordings_on;
use crate::{Actor, Catalog, Error, Result, Source, identity::recording_access};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordingGroupProof {
    MusicbrainzRecording,
    Isrc,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingCopy {
    pub recording_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub proof: Option<RecordingGroupProof>,
    pub canonical: bool,
    pub available: bool,
    pub sources: Vec<Source>,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingCopies {
    pub canonical_id: String,
    pub copies: Vec<RecordingCopy>,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingSplit {
    pub recording_id: String,
    pub separate: bool,
}

impl RecordingGroupProof {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::MusicbrainzRecording => "musicbrainz_recording",
            Self::Isrc => "isrc",
        }
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        match text {
            "musicbrainz_recording" => Some(Self::MusicbrainzRecording),
            "isrc" => Some(Self::Isrc),
            _ => None,
        }
    }
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn recording_copies(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingCopies> {
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        recording_access(&mut connection, actor, recording).await?;
        let canonical: String = sqlx::query_scalar(
            "SELECT canonical_id FROM recording_group_members WHERE user_id=? AND recording_id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_optional(&mut *connection)
        .await?
        .unwrap_or_else(|| recording.to_owned());
        if canonical != recording
            && recording_access(&mut connection, actor, &canonical)
                .await
                .is_err()
        {
            return Ok(RecordingCopies {
                canonical_id: recording.to_owned(),
                copies: Vec::new(),
            });
        }
        let members: Vec<(String, String)> = sqlx::query_as(
            "SELECT recording_id,proof FROM recording_group_members
             WHERE user_id=? AND canonical_id=? ORDER BY created_at,recording_id",
        )
        .bind(actor.id())
        .bind(&canonical)
        .fetch_all(&mut *connection)
        .await?;
        if members.is_empty() {
            return Ok(RecordingCopies {
                canonical_id: canonical,
                copies: Vec::new(),
            });
        }
        let mut ids = vec![canonical.clone()];
        ids.extend(members.iter().map(|member| member.0.clone()));
        let uuids = ids
            .iter()
            .map(|id| Uuid::parse_str(id))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| Error::Internal("Recording identity is malformed".into()))?;
        let shown = recordings_on(&mut connection, &self.host, actor, &uuids).await?;
        drop(connection);
        let mut copies = Vec::with_capacity(shown.len());
        for recording in shown {
            let proof = members
                .iter()
                .find(|member| member.0 == recording.id)
                .and_then(|member| RecordingGroupProof::parse(&member.1));
            copies.push(RecordingCopy {
                canonical: recording.id == canonical,
                sources: self.sources(actor, &recording.id).await?,
                recording_id: recording.id,
                title: recording.title,
                artist: recording.artist,
                album: recording.album,
                proof,
                available: recording.available,
            });
        }
        tracing::debug!(copies = copies.len(), "Recording copies loaded");
        Ok(RecordingCopies {
            canonical_id: canonical,
            copies,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(operation = "recording_split", account_id = actor.id()))]
    pub async fn split_recording_copy(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingSplit> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let canonical: Option<String> = sqlx::query_scalar(
            "SELECT canonical_id FROM recording_group_members WHERE user_id=? AND recording_id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(canonical) = canonical else {
            let carries: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM recording_group_members WHERE user_id=? AND canonical_id=?)",
            )
            .bind(actor.id())
            .bind(recording)
            .fetch_one(&mut *tx)
            .await?;
            if carries {
                return Err(Error::Conflict(
                    "Choose the copy that is not the same song instead of the one that carries the card"
                        .into(),
                ));
            }
            return Ok(RecordingSplit {
                recording_id: recording.to_owned(),
                separate: true,
            });
        };
        let result = sqlx::query(
            "INSERT OR IGNORE INTO recording_group_splits(user_id,recording_id,other_id)
             SELECT ?1,min(?2,peer),max(?2,peer) FROM (
                SELECT ?3 AS peer UNION SELECT recording_id FROM recording_group_members
                WHERE user_id=?1 AND canonical_id=?3 AND recording_id<>?2)
             WHERE peer<>?2",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(&canonical)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM recording_group_members WHERE user_id=? AND recording_id=?")
            .bind(actor.id())
            .bind(recording)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(
            event = "recording_split",
            separated_pairs = result.rows_affected(),
            "A recording was separated from its song"
        );
        Ok(RecordingSplit {
            recording_id: recording.to_owned(),
            separate: true,
        })
    }
}
