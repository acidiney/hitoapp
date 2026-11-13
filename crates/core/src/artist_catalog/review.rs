use super::{
    ArtistReviewContext, CreditMemberReviewContext, MetadataProvider, REQUEST_BUDGET,
    authority::{self, Credit},
    credit_target::CreditTarget,
    policy,
    provider::Connection,
};
use crate::{Actor, ArtistCatalog, Error, Result, identity::recording_access};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_review_begin", request_id = %Uuid::new_v4()))]
    pub async fn begin_artist_review(
        &self,
        actor: &Actor,
        recording: &str,
        provider: MetadataProvider,
    ) -> Result<ArtistReviewContext> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.begin_review(actor, recording, provider),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist review exceeded the time limit",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist review context captured"
        );
        result
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "credit_member_review_begin", request_id = %Uuid::new_v4()))]
    pub async fn begin_credit_member_review(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
        provider: MetadataProvider,
    ) -> Result<CreditMemberReviewContext> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            REQUEST_BUDGET,
            self.begin_member_review(actor, recording, artist, provider),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Artist review exceeded the time limit",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member review context captured"
        );
        result
    }

    async fn begin_member_review(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
        provider: MetadataProvider,
    ) -> Result<CreditMemberReviewContext> {
        let _admission = self.admit(actor.id())?;
        provider.validate()?;
        let (credit, policy_revision) = {
            let mut tx = self.store.pool.begin().await?;
            let credit = Credit::capture(&mut tx, actor, recording).await?;
            let credit =
                CreditTarget::capture_member(&mut tx, actor, recording, credit, artist, None)
                    .await?;
            let settings = policy::read(&mut tx, actor).await?;
            if !policy::selected(&settings, &provider) {
                return Err(policy::changed());
            }
            policy::approved(&mut tx, actor, &provider).await?;
            tx.commit().await?;
            (credit, settings.revision)
        };
        let connection = Connection::open(self, actor, &provider)
            .await
            .map_err(member_authority_changed)?;
        let mut tx = self.store.pool.begin().await?;
        let current = Credit::capture_target(&mut tx, actor, recording, &credit.target)
            .await
            .map_err(member_authority_changed)?;
        if current != credit {
            return Err(authority::changed());
        }
        let settings = policy::read(&mut tx, actor).await?;
        if settings.revision != policy_revision || !policy::selected(&settings, &provider) {
            return Err(policy::changed());
        }
        policy::approved(&mut tx, actor, &provider)
            .await
            .map_err(member_authority_changed)?;
        connection
            .recheck(&mut tx)
            .await
            .map_err(member_authority_changed)?;
        tx.commit().await?;
        let role = match &credit.target {
            CreditTarget::NamedMember { role, .. } => *role,
            CreditTarget::WholeCredit => {
                return Err(Error::Internal(
                    "Artist review target is not a credit member".into(),
                ));
            }
        };
        Ok(CreditMemberReviewContext {
            account_id: actor.id().into(),
            recording_id: recording.into(),
            library_id: credit.library_id.clone(),
            artist_id: credit.artist_id.clone(),
            credited_name: credit.credited_name.clone(),
            role,
            source_id: credit.source_id.clone(),
            observation_id: credit.observation_id.clone(),
            artist_operation_id: credit.credit_operation.clone(),
            access_grant_id: credit.access_grant.clone(),
            metadata_binding: credit.review_binding()?,
            provider,
            policy_revision,
            connection: connection.review_connection(),
        })
    }

    async fn begin_review(
        &self,
        actor: &Actor,
        recording: &str,
        provider: MetadataProvider,
    ) -> Result<ArtistReviewContext> {
        let _admission = self.admit(actor.id())?;
        provider.validate()?;
        let (credit, policy_revision) = {
            let mut tx = self.store.pool.begin().await?;
            let credit = Credit::capture(&mut tx, actor, recording).await?;
            let settings = policy::read(&mut tx, actor).await?;
            if !policy::selected(&settings, &provider) {
                return Err(policy::changed());
            }
            policy::approved(&mut tx, actor, &provider).await?;
            tx.commit().await?;
            (credit, settings.revision)
        };
        let connection = Connection::open(self, actor, &provider).await?;
        let context = ArtistReviewContext {
            account_id: actor.id().into(),
            recording_id: recording.into(),
            library_id: credit.library_id.clone(),
            artist_id: credit.artist_id.clone(),
            credited_name: credit.credited_name.clone(),
            source_id: credit.source_id.clone(),
            observation_id: credit.observation_id.clone(),
            artist_operation_id: credit.credit_operation.clone(),
            access_grant_id: credit.access_grant.clone(),
            metadata_binding: credit.review_binding()?,
            provider,
            policy_revision,
            connection: connection.review_connection(),
        };
        if !recheck(self, actor, &context, Some(&connection)).await? {
            return Err(policy::changed());
        }
        Ok(context)
    }
}

pub(super) async fn recheck(
    catalog: &ArtistCatalog,
    actor: &Actor,
    context: &ArtistReviewContext,
    connection: Option<&Connection>,
) -> Result<bool> {
    let mut tx = catalog.store.pool.begin().await?;
    let current = recheck_on(&mut tx, actor, context, connection).await?;
    tx.commit().await?;
    Ok(current)
}

pub(super) async fn recheck_on(
    transaction: &mut SqliteConnection,
    actor: &Actor,
    context: &ArtistReviewContext,
    connection: Option<&Connection>,
) -> Result<bool> {
    if context.account_id != actor.id() {
        return Err(Error::NotFound);
    }
    context.provider.validate()?;
    actor.check_session_on(transaction).await?;
    recording_access(transaction, actor, &context.recording_id).await?;
    let settings = policy::read(transaction, actor).await?;
    if !settings.enabled {
        return Ok(false);
    }
    if settings.revision != context.policy_revision
        || !policy::selected(&settings, &context.provider)
    {
        return Err(policy::changed());
    }
    let current = Credit::capture(transaction, actor, &context.recording_id).await?;
    if !current.matches_review(context) {
        return Err(authority::changed());
    }
    policy::approved(transaction, actor, &context.provider).await?;
    if let Some(connection) = connection {
        if connection.review_connection() != context.connection {
            return Err(authority::changed());
        }
        connection.recheck(transaction).await?;
    }
    Ok(true)
}

fn member_authority_changed(error: Error) -> Error {
    match error {
        Error::NotFound => authority::changed(),
        error => error,
    }
}
