use super::{
    ArtistAssociation, ArtistProfileAvailability, REQUEST_BUDGET, associations, authority::Credit,
    policy, provider::Connection,
};
use crate::{Actor, ArtistCatalog, Error, Result, identity::recording_access};
use hito_plugin_sdk::ArtistProfileRequest;
use std::{future::Future, time::Instant};
use uuid::Uuid;

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_profile", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_profile_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ArtistProfileAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ArtistProfileAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_artist_profile(actor, recording, association)) => {
                result.unwrap_or(Ok(ArtistProfileAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ArtistProfileAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist profile read completed"
        );
        result
    }

    async fn read_artist_profile(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
    ) -> Result<ArtistProfileAvailability> {
        let _admission = self.admit(actor.id())?;
        let (association, credit, policy_revision) = {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            recording_access(&mut tx, actor, recording).await?;
            let association = associations::load(&mut tx, actor, recording, association).await?;
            let settings = policy::read(&mut tx, actor).await?;
            if !settings.enabled {
                return Ok(ArtistProfileAvailability::Disabled);
            }
            if !policy::selected(&settings, &association.provider) {
                return Ok(ArtistProfileAvailability::Unavailable);
            }
            let credit = match Credit::capture_association(&mut tx, actor, &association).await {
                Err(Error::NotFound) => return Ok(ArtistProfileAvailability::Unavailable),
                result => result?,
            };
            if !credit.matches(&mut tx, actor, &association).await? {
                return Ok(ArtistProfileAvailability::Unavailable);
            }
            tx.commit().await?;
            (association, credit, settings.revision)
        };
        let connection = match Connection::open(self, actor, &association.provider).await {
            Err(Error::NotFound) => return Ok(ArtistProfileAvailability::Unavailable),
            result => result?,
        };
        self.recheck_artist_profile(actor, &association, &credit, policy_revision, &connection)
            .await?;
        let supported = connection.supported().await;
        self.recheck_artist_profile(actor, &association, &credit, policy_revision, &connection)
            .await?;
        match supported {
            Ok(false) => return Ok(ArtistProfileAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ArtistProfileAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let request = ArtistProfileRequest {
            artist_id: association.provider_artist_id.clone(),
        };
        let result = connection.profile(&request).await;
        self.recheck_artist_profile(actor, &association, &credit, policy_revision, &connection)
            .await?;
        let profile = match result {
            Err(Error::Upstream(_)) => return Ok(ArtistProfileAvailability::Unavailable),
            result => result?,
        };
        profile
            .validate_for(&request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist profile"))?;
        Ok(ArtistProfileAvailability::Available {
            association_id: association.id,
            profile,
        })
    }

    async fn recheck_artist_profile(
        &self,
        actor: &Actor,
        association: &ArtistAssociation,
        credit: &Credit,
        policy_revision: i64,
        provider: &Connection,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin().await?;
        credit
            .recheck(&mut tx, actor, association, policy_revision)
            .await?;
        provider.recheck(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
