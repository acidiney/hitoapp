use super::super::{
    ArtistCreditRole, AssociateRecordingArtist, MetadataProvider, associations, authority::Credit,
    policy, provider, provider::Connection,
};
use super::*;
use crate::{Actor, Catalog, Error};
use sqlx::SqliteConnection;
use std::time::Instant;

const MAX_BATCH: i64 = 100;
const MAX_ID_LENGTH: usize = 64;

#[derive(Default)]
struct Outcome {
    unchanged: i64,
    skipped_different: i64,
    skipped_unavailable: i64,
}

struct Pending {
    recording: String,
    revision: i64,
    association_id: String,
    credit: Credit,
}

impl Catalog {
    #[tracing::instrument(level="info", skip_all, fields(operation="artist_identity_credit_apply", request_id=%request.request_id))]
    pub async fn apply_artist_identity_to_credit(
        &self,
        actor: &Actor,
        artist: &str,
        request: ApplyArtistIdentityToCredit,
    ) -> Result<ArtistIdentityCreditReceipt> {
        let started = Instant::now();
        let result = self.accept_credit_identity(actor, artist, request).await;
        match &result {
            Ok(receipt) => tracing::info!(
                outcome = "accepted",
                applied = receipt.applied,
                unchanged = receipt.unchanged,
                skipped_different = receipt.skipped_different,
                skipped_unavailable = receipt.skipped_unavailable,
                complete = receipt.next_after.is_none(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist identity applied to credit"
            ),
            Err(error) => tracing::info!(
                outcome = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist identity applied to credit"
            ),
        }
        result
    }

    async fn accept_credit_identity(
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
        let digest = receipts::fingerprint("hito-artist-identity-credit:v1", artist, &request)?;
        let group = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            credit_access(&mut connection, actor, artist).await?;
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
        credit_access(&mut tx, actor, artist).await?;
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
        if !has_current_member(&mut tx, actor, &request.group_id).await? {
            return Err(Error::Conflict(
                "The chosen artist has no current reviewed recording".into(),
            ));
        }
        let mut targets: Vec<String> = sqlx::query_scalar(
            "SELECT credit.recording_id FROM account_recording_credits credit
             JOIN recordings r ON r.id=credit.recording_id
             JOIN library_members m ON m.library_id=r.library_id AND m.user_id=credit.user_id
             WHERE credit.user_id=? AND credit.artist_id=? AND credit.recording_id>?
             ORDER BY credit.recording_id LIMIT ?",
        )
        .bind(actor.id())
        .bind(artist)
        .bind(request.after.as_deref().unwrap_or_default())
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await?;
        let next_after = if targets.len() as i64 > limit {
            targets.truncate(limit as usize);
            targets.last().cloned()
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
        for recording in targets {
            if let Some(item) = plan(
                &mut tx,
                actor,
                artist,
                &request.group_id,
                &template,
                recording,
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
            choices::store(
                &mut tx,
                actor,
                &item.recording,
                item.revision,
                &item.association_id,
                &request.group_id,
                &item.credit,
                order,
            )
            .await?;
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn plan(
    tx: &mut SqliteConnection,
    actor: &Actor,
    artist: &str,
    group: &str,
    template: &AssociateRecordingArtist,
    recording: String,
    outcome: &mut Outcome,
) -> Result<Option<Pending>> {
    let credit = match Credit::capture(tx, actor, &recording).await {
        Ok(credit) if credit.artist_id == artist => credit,
        Ok(_) | Err(Error::NotFound) => {
            outcome.skipped_unavailable += 1;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let current: Option<(i64, Option<String>, bool)> = sqlx::query_as(
        "SELECT revision,CASE WHEN association_id IS NULL THEN NULL ELSE group_id END,retired
         FROM recording_artist_identity_choices WHERE user_id=? AND recording_id=?",
    )
    .bind(actor.id())
    .bind(&recording)
    .fetch_optional(&mut *tx)
    .await?;
    let revision = match &current {
        Some((_, Some(chosen), false)) if chosen == group => {
            outcome.unchanged += 1;
            return Ok(None);
        }
        Some((_, Some(_), false)) => {
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
    match associations::retain(tx, actor, &recording, &credit.library_id, &request).await {
        Ok(association) => Ok(Some(Pending {
            recording,
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

struct Group {
    provider: MetadataProvider,
    provider_artist_id: String,
}

async fn load_group(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<Group> {
    let row: Option<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT provider_kind,plugin_installation_id,provider_artist_id
         FROM account_artist_identity_groups WHERE user_id=? AND id=?",
    )
    .bind(actor.id())
    .bind(group)
    .fetch_optional(connection)
    .await?;
    let provider = match row {
        Some((kind, None, id)) if kind == "builtin_musicbrainz" => {
            (MetadataProvider::BuiltinMusicBrainz, id)
        }
        Some((kind, Some(installation_id), id)) if kind == "http_plugin" => {
            (MetadataProvider::HttpPlugin { installation_id }, id)
        }
        Some(_) => {
            return Err(Error::Internal(
                "Stored artist identity provider is invalid".into(),
            ));
        }
        None => return Err(Error::NotFound),
    };
    Ok(Group {
        provider: provider.0,
        provider_artist_id: provider.1,
    })
}

async fn credit_access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    artist: &str,
) -> Result<()> {
    let visible: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_artists a
         JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
         WHERE a.user_id=? AND a.id=?)",
    )
    .bind(actor.id())
    .bind(artist)
    .fetch_one(connection)
    .await?;
    if visible {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}

async fn has_current_member(
    connection: &mut SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM recording_artist_identity_choices c
         JOIN recordings r ON r.id=c.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=c.user_id
         WHERE c.user_id=? AND c.group_id=? AND c.retired=0 AND c.association_id IS NOT NULL)",
    )
    .bind(actor.id())
    .bind(group)
    .fetch_one(connection)
    .await?)
}
