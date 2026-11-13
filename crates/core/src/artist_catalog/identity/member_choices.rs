use super::{
    ArtistIdentitySelection, ChooseCreditMemberIdentity, CreditMemberIdentityChoiceReceipt,
    RecordingArtistIdentityState, RecordingCreditMemberIdentityChoice, choices, receipts,
};
use crate::{
    Actor, ArtistCatalog, Error, Result,
    artist_catalog::{
        associations::{self, LINKED},
        authority::{self, Credit},
        credit_target::CreditTarget,
        provider::Connection,
        review::Authority,
    },
    identity::recording_access,
};
use hito_plugin_sdk::CreditRole;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, Transaction};
use std::time::Instant;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct StoredChoice {
    revision: i64,
    role: String,
    association_id: Option<String>,
    group_id: Option<String>,
    artist_operation_id: Option<String>,
    observation_id: Option<String>,
    retired: bool,
}

impl ArtistCatalog {
    #[tracing::instrument(level="debug", skip_all, fields(operation="credit_member_identity_read", request_id=%Uuid::new_v4()))]
    pub async fn recording_credit_member_identity_choice(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
    ) -> Result<RecordingCreditMemberIdentityChoice> {
        let started = Instant::now();
        let result = self.read_member_identity(actor, recording, artist).await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member identity loaded"
        );
        result
    }

    async fn read_member_identity(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
    ) -> Result<RecordingCreditMemberIdentityChoice> {
        actor.require_foreground_session()?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let stored: Option<StoredChoice> = sqlx::query_as(
            "SELECT revision,role,association_id,group_id,artist_operation_id,observation_id,retired
             FROM recording_artist_member_identity_choices WHERE user_id=? AND recording_id=? AND artist_id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(artist)
        .fetch_optional(&mut *tx)
        .await?;
        let (revision, role, state) = if let Some(stored) = stored {
            let role = member_role(&stored.role)?;
            let state = read_state(&mut tx, actor, recording, artist, role, &stored).await?;
            (stored.revision, role, state)
        } else {
            let role: String = sqlx::query_scalar(
                "SELECT member.role FROM account_recording_credit_members member
                 JOIN recordings recording ON recording.id=member.recording_id
                 JOIN account_artists artist ON artist.user_id=member.user_id
                    AND artist.id=member.artist_id AND artist.library_id=recording.library_id
                 WHERE member.user_id=? AND member.recording_id=? AND member.artist_id=?",
            )
            .bind(actor.id())
            .bind(recording)
            .bind(artist)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
            (
                0,
                member_role(&role)?,
                RecordingArtistIdentityState::Unselected,
            )
        };
        let access: (String, String) = sqlx::query_as(
            "SELECT recording.library_id,grant.id FROM recordings recording
             JOIN library_access_grants grant ON grant.library_id=recording.library_id AND grant.user_id=?
             WHERE recording.id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "credit_member_identity_read",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
            "Owned authority snapshot checkpoint"
        );
        tx.commit().await?;
        let mut fresh = self.store.pool.acquire().await?;
        actor.check_session_on(&mut fresh).await?;
        recording_access(&mut fresh, actor, recording).await?;
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recordings recording
             JOIN library_access_grants grant ON grant.library_id=recording.library_id AND grant.user_id=?
             WHERE recording.id=? AND recording.library_id=? AND grant.id=?)",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(&access.0)
        .bind(&access.1)
        .fetch_one(&mut *fresh)
        .await?;
        if !current {
            return Err(Error::NotFound);
        }
        actor.check_session_on(&mut fresh).await?;
        Ok(RecordingCreditMemberIdentityChoice {
            recording_id: recording.into(),
            artist_id: artist.into(),
            role,
            revision,
            state,
        })
    }

    #[tracing::instrument(level="info", skip_all, fields(operation="credit_member_identity_choose", request_id=%request.request_id))]
    pub async fn choose_credit_member_identity(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
        request: ChooseCreditMemberIdentity,
    ) -> Result<CreditMemberIdentityChoiceReceipt> {
        let started = Instant::now();
        let result = self
            .accept_member_identity(actor, recording, artist, request)
            .await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member identity choice finished"
        );
        result
    }

    async fn accept_member_identity(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
        request: ChooseCreditMemberIdentity,
    ) -> Result<CreditMemberIdentityChoiceReceipt> {
        if request.request_id.is_nil()
            || request.association_id.is_nil()
            || request.expected_revision < 0
        {
            return Err(Error::Invalid(
                "Use a nonempty request and association with a nonnegative revision".into(),
            ));
        }
        if request.context.account_id != actor.id()
            || request.context.recording_id != recording
            || request.context.artist_id != artist
        {
            return Err(Error::NotFound);
        }
        actor.require_foreground_session()?;
        let digest = receipts::fingerprint(
            "hito-credit-member-identity-choose:v1",
            recording,
            &(artist, &request),
        )?;
        let authority = Authority::Member(&request.context);
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            recording_access(&mut connection, actor, recording).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
            if !authority.recheck_on(&mut connection, actor, None).await? {
                return Err(authority::changed());
            }
        }
        let connection = Connection::open(self, actor, &request.context.provider)
            .await
            .map_err(|error| match error {
                Error::NotFound => authority::changed(),
                error => error,
            })?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let revision: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM recording_artist_member_identity_choices
             WHERE user_id=? AND recording_id=? AND artist_id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(artist)
        .fetch_optional(&mut *tx)
        .await?;
        if revision.unwrap_or(0) != request.expected_revision {
            return Err(Error::Conflict(
                "The artist identity choice changed. Review its current revision".into(),
            ));
        }
        if !authority
            .recheck_on(&mut tx, actor, Some(&connection))
            .await?
        {
            return Err(authority::changed());
        }
        let association = associations::load(
            &mut tx,
            actor,
            recording,
            &request.association_id.to_string(),
        )
        .await?;
        let target = CreditTarget::NamedMember {
            artist_id: artist.into(),
            role: request.context.role,
        };
        let credit = Credit::capture_target(&mut tx, actor, recording, &target).await?;
        if !credit.matches(&mut tx, actor, &association).await?
            || association.provider != request.context.provider
        {
            return Err(Error::Conflict(
                "The reviewed artist association changed".into(),
            ));
        }
        let revision = request.expected_revision.checked_add(1).ok_or_else(|| {
            Error::Conflict("Artist identity choice revision is exhausted".into())
        })?;
        let (kind, installation) = authority::provider_columns(&association.provider);
        let group = choices::ensure_group(
            &mut tx,
            actor,
            kind,
            installation,
            &association.provider_artist_id,
        )
        .await?;
        let receipt = CreditMemberIdentityChoiceReceipt {
            request_id: request.request_id,
            recording_id: recording.into(),
            artist_id: artist.into(),
            role: request.context.role,
            revision,
            selection: Some(ArtistIdentitySelection {
                group_id: group.clone(),
                association_id: association.id.clone(),
            }),
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        sqlx::query(
            "INSERT INTO recording_artist_member_identity_choices(user_id,recording_id,artist_id,role,revision,
                association_id,group_id,artist_operation_id,observation_id,selection_order)
             VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(user_id,recording_id,artist_id) DO UPDATE SET
                role=excluded.role,revision=excluded.revision,association_id=excluded.association_id,
                group_id=excluded.group_id,artist_operation_id=excluded.artist_operation_id,
                observation_id=excluded.observation_id,retired=0,selected_at=unixepoch(),selection_order=excluded.selection_order",
        )
        .bind(actor.id())
        .bind(recording)
        .bind(artist)
        .bind(role_name(request.context.role))
        .bind(revision)
        .bind(&association.id)
        .bind(&group)
        .bind(&credit.credit_operation)
        .bind(&credit.observation_id)
        .bind(order)
        .execute(&mut *tx)
        .await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn read_state(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    artist: &str,
    role: CreditRole,
    stored: &StoredChoice,
) -> Result<RecordingArtistIdentityState> {
    let (Some(association), Some(group)) = (&stored.association_id, &stored.group_id) else {
        return Ok(RecordingArtistIdentityState::Unselected);
    };
    let association = match associations::load(connection, actor, recording, association).await {
        Ok(association) => association,
        Err(Error::NotFound) => return Ok(RecordingArtistIdentityState::Unselected),
        Err(error) => return Err(error),
    };
    let selection = ArtistIdentitySelection {
        group_id: group.clone(),
        association_id: association.id.clone(),
    };
    if !stored.retired {
        let target = CreditTarget::NamedMember {
            artist_id: artist.into(),
            role,
        };
        match Credit::capture_target(connection, actor, recording, &target).await {
            Ok(credit)
                if stored.observation_id.as_deref() == Some(credit.observation_id.as_str())
                    && stored.artist_operation_id == credit.credit_operation =>
            {
                if credit.matches(connection, actor, &association).await? {
                    return Ok(RecordingArtistIdentityState::Selected(selection));
                }
            }
            Ok(_) | Err(Error::NotFound | Error::Conflict(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(RecordingArtistIdentityState::NeedsReview(selection))
}

fn member_role(role: &str) -> Result<CreditRole> {
    match role {
        "main" => Ok(CreditRole::Main),
        "featured" => Ok(CreditRole::Featured),
        _ => Err(Error::Internal(
            "Stored artist credit member is invalid".into(),
        )),
    }
}

fn role_name(role: CreditRole) -> &'static str {
    match role {
        CreditRole::Main => "main",
        CreditRole::Featured => "featured",
    }
}

pub(super) fn current_choices() -> String {
    format!(
        "FROM recording_artist_member_identity_choices choice
         JOIN recordings r ON r.id=choice.recording_id
         JOIN libraries library ON library.id=r.library_id
         JOIN library_access_grants access ON access.library_id=r.library_id AND access.user_id=choice.user_id
         JOIN account_recording_credits whole ON whole.user_id=choice.user_id AND whole.recording_id=r.id
         JOIN account_artists whole_artist ON whole_artist.user_id=whole.user_id
            AND whole_artist.id=whole.artist_id AND whole_artist.library_id=r.library_id
         JOIN account_recording_credit_members credit ON credit.user_id=choice.user_id
            AND credit.recording_id=choice.recording_id AND credit.artist_id=choice.artist_id AND credit.role=choice.role
         JOIN account_artists artist ON artist.user_id=choice.user_id AND artist.id=choice.artist_id AND artist.library_id=r.library_id
         JOIN recording_artist_associations assoc ON assoc.id=choice.association_id AND assoc.user_id=choice.user_id
            AND assoc.recording_id=choice.recording_id AND assoc.artist_id=choice.artist_id
            AND assoc.credit_target=choice.credit_target AND assoc.observation_id=choice.observation_id
         JOIN metadata_observations observation ON observation.id=r.metadata_observation_id
            AND observation.recording_id=r.id AND observation.source_id IS assoc.source_id
         LEFT JOIN personal_recording_metadata personal ON personal.recording_id=r.id AND personal.user_id=choice.user_id
         LEFT JOIN sources source ON source.id=observation.source_id AND source.recording_id=r.id
         LEFT JOIN source_metadata source_state ON source_state.source_id=source.id AND source_state.observation_id=observation.id
         LEFT JOIN provider_items primary_binding ON primary_binding.id=r.metadata_binding_id
            AND primary_binding.id=observation.provider_binding_id AND primary_binding.recording_id=r.id
            AND primary_binding.installation_id=observation.plugin_installation_id
            AND primary_binding.provider_recording_id=observation.provider_recording_id
            AND primary_binding.role='primary' AND primary_binding.retired=0
         LEFT JOIN plugin_installations origin ON origin.id=primary_binding.installation_id AND origin.user_id=choice.user_id
         WHERE choice.retired=0 AND choice.association_id IS NOT NULL
            AND r.metadata_observation_id=choice.observation_id AND personal.artist_operation_id IS choice.artist_operation_id
            AND ((observation.source_id IS NOT NULL AND source.id IS NOT NULL AND source_state.observation_id=observation.id)
                OR (observation.source_id IS NULL AND observation.origin='provider_response'
                    AND primary_binding.id IS NOT NULL AND origin.id IS NOT NULL AND library.owner_id=choice.user_id))
            AND {LINKED} AND choice.user_id="
    )
}

pub(crate) async fn retire_changed(
    tx: &mut Transaction<'_, Sqlite>,
    mut targets: QueryBuilder<'static, Sqlite>,
) -> Result<()> {
    targets.push(
        "UPDATE recording_artist_member_identity_choices AS choice
         SET retired=1,revision=revision+1 WHERE retired=0 AND association_id IS NOT NULL
            AND (choice.user_id,choice.recording_id) IN (SELECT user_id,recording_id FROM targets)
            AND (NOT EXISTS (SELECT 1 FROM account_recording_credit_members member
                    WHERE member.user_id=choice.user_id AND member.recording_id=choice.recording_id
                        AND member.artist_id=choice.artist_id AND member.role=choice.role)
                OR EXISTS (SELECT 1 FROM recordings r
                    LEFT JOIN personal_recording_metadata personal ON personal.user_id=choice.user_id AND personal.recording_id=r.id
                    WHERE r.id=choice.recording_id AND (r.metadata_observation_id IS NOT choice.observation_id
                        OR personal.artist_operation_id IS NOT choice.artist_operation_id)))",
    );
    let retired = targets.build().execute(&mut **tx).await?.rows_affected();
    if retired > 0 {
        tracing::debug!(retired, "Credit member identity choices retired");
    }
    Ok(())
}
