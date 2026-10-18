use super::{
    ArtistAssociation, ArtistDiscographyAvailability, REQUEST_BUDGET, ReadArtistDiscography,
    associations, authority::Credit, continuation, policy, provider::Connection,
};
use crate::{Actor, Catalog, Error, Result, identity::recording_access};
use hito_plugin_sdk::{ArtistDiscographyRequest, MAX_CATALOG_PAGE_ITEMS};
use std::{future::Future, time::Instant};
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_discography", request_id = %Uuid::new_v4()))]
    pub async fn recording_artist_discography_until(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadArtistDiscography,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ArtistDiscographyAvailability> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(ArtistDiscographyAvailability::Cancelled),
            result = tokio::time::timeout_at(deadline, self.read_artist_discography(actor, recording, association, request)) => {
                result.unwrap_or(Ok(ArtistDiscographyAvailability::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(ArtistDiscographyAvailability::outcome)
                .unwrap_or_else(|error| error.code()),
            item_count = match &result {
                Ok(ArtistDiscographyAvailability::Available { page, .. }) => page.items.len(),
                _ => 0,
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist discography read completed"
        );
        result
    }

    async fn read_artist_discography(
        &self,
        actor: &Actor,
        recording: &str,
        association: &str,
        request: ReadArtistDiscography,
    ) -> Result<ArtistDiscographyAvailability> {
        if !(1..=MAX_CATALOG_PAGE_ITEMS).contains(&request.limit)
            || request.continuation.as_ref().is_some_and(|token| {
                token.is_empty() || token.len() > continuation::MAX_TOKEN_BYTES || !token.is_ascii()
            })
        {
            return Err(Error::Invalid(
                "Use a valid catalog page limit and continuation".into(),
            ));
        }
        let _admission = self.artist_catalog.admit(actor.id())?;
        let (association, credit, policy_revision) = {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            recording_access(&mut tx, actor, recording).await?;
            let association = associations::load(&mut tx, actor, recording, association).await?;
            let settings = policy::read(&mut tx, actor).await?;
            if !settings.enabled {
                return Ok(ArtistDiscographyAvailability::Disabled);
            }
            if !policy::selected(&settings, &association.provider) {
                return Ok(ArtistDiscographyAvailability::Unavailable);
            }
            let credit = match Credit::capture(&mut tx, actor, recording).await {
                Err(Error::NotFound) => return Ok(ArtistDiscographyAvailability::Unavailable),
                result => result?,
            };
            if !credit.matches(&association) {
                return Ok(ArtistDiscographyAvailability::Unavailable);
            }
            tx.commit().await?;
            (association, credit, settings.revision)
        };
        let connection = match Connection::open(self, actor, &association.provider).await {
            Err(Error::NotFound) => return Ok(ArtistDiscographyAvailability::Unavailable),
            result => result?,
        };
        self.recheck_artist_discography(actor, &association, &credit, policy_revision, &connection)
            .await?;
        let binding =
            continuation::binding(actor, &association, &credit, policy_revision, &connection)?;
        let provider_request = match request.continuation {
            Some(token) => {
                self.artist_catalog
                    .continuations
                    .open(&token, binding, request.limit)?
            }
            None => ArtistDiscographyRequest {
                artist_id: association.provider_artist_id.clone(),
                cursor: None,
                snapshot: None,
                limit: request.limit,
            },
        };
        if provider_request.artist_id != association.provider_artist_id {
            return Err(Error::Invalid(
                "Use a valid catalog continuation or start again".into(),
            ));
        }
        let supported = connection.discography_supported().await;
        self.recheck_artist_discography(actor, &association, &credit, policy_revision, &connection)
            .await?;
        match supported {
            Ok(false) => return Ok(ArtistDiscographyAvailability::Unsupported),
            Err(Error::Upstream(_)) => return Ok(ArtistDiscographyAvailability::Unavailable),
            result => {
                result?;
            }
        }
        let result = connection.discography(&provider_request).await;
        self.recheck_artist_discography(actor, &association, &credit, policy_revision, &connection)
            .await?;
        let page = match result {
            Err(Error::Upstream(_)) => return Ok(ArtistDiscographyAvailability::Unavailable),
            result => result?,
        };
        page.validate_for(&provider_request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist discography"))?;
        let continuation = page
            .next_cursor
            .as_ref()
            .map(|cursor| {
                self.artist_catalog.continuations.seal(
                    binding,
                    ArtistDiscographyRequest {
                        artist_id: association.provider_artist_id.clone(),
                        cursor: Some(cursor.clone()),
                        snapshot: page.snapshot.clone(),
                        limit: provider_request.limit,
                    },
                )
            })
            .transpose()?;
        Ok(ArtistDiscographyAvailability::Available {
            association_id: association.id,
            page,
            continuation,
        })
    }

    async fn recheck_artist_discography(
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
