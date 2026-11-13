use super::{
    REQUEST_BUDGET,
    authority::{ReviewedRead, ReviewedReadState},
};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::{CatalogReleaseGroup, ReleaseGroupRequest};
use serde::{Deserialize, Serialize};
use std::{future::Future, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadReleaseGroup {
    pub release_group_id: String,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReleaseGroupAvailability {
    Available {
        association_id: String,
        group: CatalogReleaseGroup,
    },
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl ReleaseGroupAvailability {
    pub(crate) fn outcome(&self) -> &'static str {
        match self {
            Self::Available { .. } => "available",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_release_group", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_release_group_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadReleaseGroup,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ReleaseGroupAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ReleaseGroupAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_release_group(actor, recording, association, request)) => {
                result.unwrap_or(Ok(ReleaseGroupAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ReleaseGroupAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist release-group read completed"
        );
        result
    }

    async fn read_release_group(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadReleaseGroup,
    ) -> Result<ReleaseGroupAvailability> {
        let request = ReleaseGroupRequest {
            release_group_id: request.release_group_id,
        };
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid release-group request".into()))?;
        let read =
            match ReviewedRead::capture_release_group(self, actor, recording, association).await? {
                ReviewedReadState::Available(read) => read,
                ReviewedReadState::Disabled => return Ok(ReleaseGroupAvailability::Disabled),
                ReviewedReadState::Unavailable => return Ok(ReleaseGroupAvailability::Unavailable),
            };
        let supported = read.connection.release_group_supported().await;
        read.recheck().await?;
        match supported {
            Ok(false) => return Ok(ReleaseGroupAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ReleaseGroupAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let result = read.connection.release_group(&request).await;
        read.recheck().await?;
        let group = match result {
            Err(Error::Upstream(_)) => return Ok(ReleaseGroupAvailability::Unavailable),
            result => result?,
        };
        group
            .validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned an invalid release group"))?;
        Ok(ReleaseGroupAvailability::Available {
            association_id: read.association.id,
            group,
        })
    }
}
