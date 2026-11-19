use super::credit::{
    MAX_BATCH, MAX_ID_LENGTH, Outcome, comparable, credit_name, current_member_names, load_group,
};
use super::member_choices::{current_choices, role_name};
use super::{
    ApplyArtistIdentityToCredit, ArtistIdentityCreditReceipt, MemberArtistIdentity,
    MemberArtistIdentityChosen, receipts,
};
use crate::{
    Actor, ArtistCatalog, Error, Result,
    artist_catalog::{
        ArtistCreditRole, AssociateRecordingArtist, associations, authority::Credit,
        credit_target::CreditTarget, policy, provider, provider::Connection,
    },
};
use hito_plugin_sdk::CreditRole;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use std::time::Instant;

struct Pending {
    recording: String,
    role: CreditRole,
    revision: i64,
    association_id: String,
    credit: Credit,
}

fn stored_role(role: &str) -> Option<CreditRole> {
    match role {
        "main" => Some(CreditRole::Main),
        "featured" => Some(CreditRole::Featured),
        _ => None,
    }
}

impl ArtistCatalog {
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(operation = "member_artist_identity_read")
    )]
    pub async fn member_artist_identity(
        &self,
        actor: &Actor,
        artist: &str,
    ) -> Result<MemberArtistIdentity> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        credit_name(&mut tx, actor, artist).await?;
        let mut query: QueryBuilder<'_, Sqlite> = QueryBuilder::new(format!(
            "SELECT choice.group_id AS group_id, count(DISTINCT choice.recording_id) AS tracks {}",
            current_choices()
        ));
        query
            .push_bind(actor.id().to_owned())
            .push(" AND choice.artist_id=")
            .push_bind(artist.to_owned())
            .push(" GROUP BY choice.group_id ORDER BY tracks DESC, choice.group_id LIMIT 20");
        let chosen: Vec<MemberArtistIdentityChosen> =
            query.build_query_as().fetch_all(&mut *tx).await?;
        let appearances: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT member.recording_id) FROM account_recording_credit_members member
             JOIN recordings r ON r.id=member.recording_id
             JOIN library_members m ON m.library_id=r.library_id AND m.user_id=member.user_id
             WHERE member.user_id=? AND member.artist_id=?",
        )
        .bind(actor.id())
        .bind(artist)
        .fetch_one(&mut *tx)
        .await?;
        let decided: i64 = chosen.iter().map(|chosen| chosen.tracks).sum();
        tx.commit().await?;
        Ok(MemberArtistIdentity {
            artist_id: artist.into(),
            chosen,
            undecided_tracks: (appearances - decided).max(0),
        })
    }

    #[tracing::instrument(level="info", skip_all, fields(operation="artist_identity_member_credit_apply", request_id=%request.request_id))]
    pub async fn apply_artist_identity_to_member_credits(
        &self,
        actor: &Actor,
        artist: &str,
        request: ApplyArtistIdentityToCredit,
    ) -> Result<ArtistIdentityCreditReceipt> {
        let started = Instant::now();
        let result = self
            .accept_member_credit_identity(actor, artist, request)
            .await;
        match &result {
            Ok(receipt) => tracing::info!(
                outcome = "accepted",
                applied = receipt.applied,
                unchanged = receipt.unchanged,
                skipped_different = receipt.skipped_different,
                skipped_unavailable = receipt.skipped_unavailable,
                complete = receipt.next_after.is_none(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist identity applied to member credits"
            ),
            Err(error) => tracing::info!(
                outcome = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist identity applied to member credits"
            ),
        }
        result
    }

    async fn accept_member_credit_identity(
        &self,
        actor: &Actor,
        artist: &str,
        request: ApplyArtistIdentityToCredit,
    ) -> Result<ArtistIdentityCreditReceipt> {
        let limit = request.limit.unwrap_or(MAX_BATCH);
        if request.request_id.is_nil()
            || request.group_id.is_empty()
            || request.group_id.len() > MAX_ID_LENGTH
            || request
                .after
                .as_ref()
                .is_some_and(|after| after.len() > MAX_ID_LENGTH)
            || !(1..=MAX_BATCH).contains(&limit)
        {
            return Err(Error::Invalid(
                "Use a nonempty request and chosen artist, and a batch of 1 to 100 recordings"
                    .into(),
            ));
        }
        let digest =
            receipts::fingerprint("hito-artist-identity-member-credit:v1", artist, &request)?;
        let group = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            credit_name(&mut connection, actor, artist).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
            load_group(&mut connection, actor, &request.group_id).await?
        };
        provider::validate_artist_id(&group.provider, &group.provider_artist_id)?;
        let connection = Connection::open(self, actor, &group.provider).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let member_name = credit_name(&mut tx, actor, artist).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &group.provider) {
            return Err(policy::changed());
        }
        policy::approved(&mut tx, actor, &group.provider).await?;
        connection.recheck(&mut tx).await?;
        let mut reviewed = current_member_names(&mut tx, actor, &request.group_id).await?;
        reviewed.extend(reviewed_member_names(&mut tx, actor, &request.group_id).await?);
        if reviewed.is_empty() {
            return Err(Error::Conflict(
                "The chosen artist has no current reviewed recording".into(),
            ));
        }
        let wanted = comparable(&member_name);
        if !reviewed.iter().any(|name| comparable(name) == wanted) {
            return Err(Error::Conflict(
                "The chosen artist was not reviewed for a recording with this name".into(),
            ));
        }
        let mut targets: Vec<(String, String)> = sqlx::query_as(
            "SELECT member.recording_id, CASE WHEN max(member.role='main') THEN 'main' ELSE 'featured' END FROM account_recording_credit_members member
             JOIN recordings r ON r.id=member.recording_id
             JOIN library_members m ON m.library_id=r.library_id AND m.user_id=member.user_id
             WHERE member.user_id=? AND member.artist_id=? AND member.recording_id>?
             GROUP BY member.recording_id ORDER BY member.recording_id LIMIT ?",
        )
        .bind(actor.id())
        .bind(artist)
        .bind(request.after.as_deref().unwrap_or_default())
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await?;
        let next_after = if targets.len() as i64 > limit {
            targets.truncate(limit as usize);
            targets.last().map(|(recording, _)| recording.clone())
        } else {
            None
        };
        let template = AssociateRecordingArtist {
            artist_id: artist.into(),
            source_id: None,
            observation_id: String::new(),
            role: ArtistCreditRole::RecordingArtist,
            provider: group.provider.clone(),
            provider_artist_id: group.provider_artist_id.clone(),
            policy_revision: settings.revision,
        };
        let mut outcome = Outcome::default();
        let mut pending = Vec::new();
        for (recording, role) in targets {
            let Some(role) = stored_role(&role) else {
                outcome.skipped_unavailable += 1;
                continue;
            };
            if let Some(item) = plan(
                &mut tx,
                actor,
                &request.group_id,
                &template,
                recording,
                role,
                &mut outcome,
            )
            .await?
            {
                pending.push(item);
            }
        }
        let receipt = ArtistIdentityCreditReceipt {
            request_id: request.request_id,
            artist_id: artist.into(),
            group_id: request.group_id.clone(),
            applied: pending.len() as i64,
            unchanged: outcome.unchanged,
            skipped_different: outcome.skipped_different,
            skipped_unavailable: outcome.skipped_unavailable,
            next_after,
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        for item in &pending {
            sqlx::query(
                "INSERT INTO recording_artist_member_identity_choices(user_id,recording_id,artist_id,role,revision,
                    association_id,group_id,artist_operation_id,observation_id,selection_order)
                 VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(user_id,recording_id,artist_id) DO UPDATE SET
                    role=excluded.role,revision=excluded.revision,association_id=excluded.association_id,
                    group_id=excluded.group_id,artist_operation_id=excluded.artist_operation_id,
                    observation_id=excluded.observation_id,retired=0,selected_at=unixepoch(),selection_order=excluded.selection_order",
            )
            .bind(actor.id())
            .bind(&item.recording)
            .bind(artist)
            .bind(role_name(item.role))
            .bind(item.revision)
            .bind(&item.association_id)
            .bind(&request.group_id)
            .bind(&item.credit.credit_operation)
            .bind(&item.credit.observation_id)
            .bind(order)
            .execute(&mut *tx)
            .await?;
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn reviewed_member_names(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<Vec<String>> {
    let mut query: QueryBuilder<'_, Sqlite> =
        QueryBuilder::new(format!("SELECT DISTINCT artist.name {}", current_choices()));
    query
        .push_bind(actor.id().to_owned())
        .push(" AND choice.group_id=")
        .push_bind(group.to_owned())
        .push(" LIMIT 200");
    Ok(query.build_query_scalar().fetch_all(connection).await?)
}

async fn plan(
    tx: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
    template: &AssociateRecordingArtist,
    recording: String,
    role: CreditRole,
    outcome: &mut Outcome,
) -> Result<Option<Pending>> {
    let target = CreditTarget::NamedMember {
        artist_id: template.artist_id.clone(),
        role,
    };
    let credit = match Credit::capture_target(tx, actor, &recording, &target).await {
        Ok(credit) => credit,
        Err(Error::NotFound | Error::Conflict(_)) => {
            outcome.skipped_unavailable += 1;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let current: Option<(i64, Option<String>, bool)> = sqlx::query_as(
        "SELECT revision,CASE WHEN association_id IS NULL THEN NULL ELSE group_id END,retired
         FROM recording_artist_member_identity_choices WHERE user_id=? AND recording_id=? AND artist_id=?",
    )
    .bind(actor.id())
    .bind(&recording)
    .bind(&template.artist_id)
    .fetch_optional(&mut *tx)
    .await?;
    let revision = match &current {
        Some((_, Some(chosen), false)) if chosen == group => {
            outcome.unchanged += 1;
            return Ok(None);
        }
        Some((_, Some(chosen), _)) if chosen != group => {
            outcome.skipped_different += 1;
            return Ok(None);
        }
        Some((revision, _, _)) => *revision,
        None => 0,
    };
    let Some(revision) = revision.checked_add(1) else {
        outcome.skipped_unavailable += 1;
        return Ok(None);
    };
    let request = AssociateRecordingArtist {
        source_id: credit.source_id.clone(),
        observation_id: credit.observation_id.clone(),
        artist_id: template.artist_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: template.provider.clone(),
        provider_artist_id: template.provider_artist_id.clone(),
        policy_revision: template.policy_revision,
    };
    if associations::unlinked_match(tx, actor, &recording, &request).await? {
        outcome.skipped_different += 1;
        return Ok(None);
    }
    match associations::retain_target(tx, actor, &recording, &credit.library_id, &request, &target)
        .await
    {
        Ok(association) => Ok(Some(Pending {
            recording,
            role,
            revision,
            association_id: association.id,
            credit,
        })),
        Err(Error::Conflict(_)) => {
            outcome.skipped_unavailable += 1;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
