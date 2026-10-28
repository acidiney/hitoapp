use super::{ArtistMatchReceipt, MatchArtistsFromTags};
use crate::catalog::artist_catalog::{
    ArtistCreditRole, AssociateRecordingArtist, MetadataProvider, associations,
    authority::{Credit, provider_columns},
    identity::{choices, receipts},
    policy, provider,
};
use crate::{Actor, Catalog, Error, Result};
use sqlx::{Connection, SqliteConnection};
use std::{collections::BTreeSet, time::Instant};
use uuid::Uuid;

const MAX_BATCH: i64 = 100;
const MAX_ID_LENGTH: usize = 64;
const EVIDENCE_KIND: &str = "file_tag_artist_id";

enum Evidence {
    None,
    Ambiguous,
    Artist(String),
}

struct Pending {
    recording: String,
    association: String,
    created: bool,
    credit: Credit,
}

#[derive(Default)]
struct Outcome {
    ambiguous: i64,
    declined: i64,
}

impl Catalog {
    /// Records reviewed built-in artist associations and choices for recordings whose own file
    /// tags name exactly one performer artist identifier. Names never contribute evidence, and
    /// recordings the account has already decided about are left alone.
    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_match_tags", request_id = %request.request_id))]
    pub async fn match_artists_from_tags(
        &self,
        actor: &Actor,
        request: MatchArtistsFromTags,
    ) -> Result<ArtistMatchReceipt> {
        let started = Instant::now();
        let result = self.accept_tag_matches(actor, request).await;
        match &result {
            Ok(receipt) => tracing::info!(
                outcome = "accepted",
                matched = receipt.matched,
                skipped_ambiguous = receipt.skipped_ambiguous,
                skipped_declined = receipt.skipped_declined,
                complete = receipt.next_after.is_none(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist tag matching finished"
            ),
            Err(error) => tracing::info!(
                outcome = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Artist tag matching finished"
            ),
        }
        result
    }

    async fn accept_tag_matches(
        &self,
        actor: &Actor,
        request: MatchArtistsFromTags,
    ) -> Result<ArtistMatchReceipt> {
        let limit = request.limit.unwrap_or(MAX_BATCH);
        if request.request_id.is_nil()
            || request
                .after
                .as_ref()
                .is_some_and(|after| after.len() > MAX_ID_LENGTH)
            || !(1..=MAX_BATCH).contains(&limit)
        {
            return Err(Error::Invalid(
                "Use a nonempty request and a batch of 1 to 100 recordings".into(),
            ));
        }
        let digest = receipts::fingerprint("hito-artist-match-tags:v1", "", &request)?;
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let empty = |next_after| ArtistMatchReceipt {
            request_id: request.request_id,
            matched: 0,
            skipped_ambiguous: 0,
            skipped_declined: 0,
            next_after,
        };
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Ok(empty(None));
        }
        let mut candidates: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT r.id,o.tags FROM recordings r
             JOIN library_members m ON m.library_id=r.library_id AND m.user_id=?
             JOIN metadata_observations o ON o.id=r.metadata_observation_id AND o.recording_id=r.id
             WHERE r.id>? AND o.source_id IS NOT NULL
                AND json_array_length(json_extract(o.tags,'$.musicbrainz_artist_tags'))>0
                AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                    WHERE c.user_id=m.user_id AND c.recording_id=r.id)
             ORDER BY r.id LIMIT ?",
        )
        .bind(actor.id())
        .bind(request.after.as_deref().unwrap_or_default())
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await?;
        let next_after = if candidates.len() as i64 > limit {
            candidates.truncate(limit as usize);
            candidates.last().map(|(id, _)| id.clone())
        } else {
            None
        };
        let mut outcome = Outcome::default();
        let mut pending = Vec::new();
        for (recording, tags) in candidates {
            let artist = match evidence(tags.as_deref()) {
                Evidence::None => continue,
                Evidence::Ambiguous => {
                    outcome.ambiguous += 1;
                    continue;
                }
                Evidence::Artist(artist) => artist,
            };
            if let Some(item) = plan(
                &mut tx,
                actor,
                recording,
                artist,
                settings.revision,
                &mut outcome,
            )
            .await?
            {
                pending.push(item);
            }
        }
        let mut receipt = empty(next_after);
        receipt.matched = pending.len() as i64;
        receipt.skipped_ambiguous = outcome.ambiguous;
        receipt.skipped_declined = outcome.declined;
        if pending.is_empty() {
            return Ok(receipt);
        }
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        for item in &pending {
            let artist: String = sqlx::query_scalar(
                "SELECT provider_artist_id FROM recording_artist_associations WHERE id=?",
            )
            .bind(&item.association)
            .fetch_one(&mut *tx)
            .await?;
            let (kind, installation) = provider_columns(&MetadataProvider::BuiltinMusicBrainz);
            let group = choices::ensure_group(&mut tx, actor, kind, installation, &artist).await?;
            choices::store(
                &mut tx,
                actor,
                &item.recording,
                1,
                &item.association,
                &group,
                &item.credit,
                order,
            )
            .await?;
            if item.created {
                sqlx::query("INSERT INTO artist_association_matches(association_id,evidence_kind,receipt_order) VALUES(?,?,?)")
                    .bind(&item.association).bind(EVIDENCE_KIND).bind(order).execute(&mut *tx).await?;
            }
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn plan(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: String,
    artist: String,
    policy_revision: i64,
    outcome: &mut Outcome,
) -> Result<Option<Pending>> {
    let credit = match Credit::capture(tx, actor, &recording).await {
        Ok(credit) if credit.credit_operation.is_none() => credit,
        Ok(_) | Err(Error::NotFound) => {
            outcome.declined += 1;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let request = AssociateRecordingArtist {
        artist_id: credit.artist_id.clone(),
        source_id: credit.source_id.clone(),
        observation_id: credit.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: MetadataProvider::BuiltinMusicBrainz,
        provider_artist_id: artist,
        policy_revision,
    };
    if associations::unlinked_match(tx, actor, &recording, &request).await? {
        outcome.declined += 1;
        return Ok(None);
    }
    let created = associations::existing(tx, actor, &recording, &request)
        .await?
        .is_none();
    let mut attempt = tx.begin().await?;
    match associations::retain(
        &mut attempt,
        actor,
        &recording,
        &credit.library_id,
        &request,
    )
    .await
    {
        Ok(association) => {
            attempt.commit().await?;
            Ok(Some(Pending {
                recording,
                association: association.id,
                created,
                credit,
            }))
        }
        Err(Error::Conflict(_) | Error::NotFound) => {
            attempt.rollback().await?;
            outcome.declined += 1;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn evidence(tags: Option<&str>) -> Evidence {
    let Some(tags) = tags.and_then(|tags| serde_json::from_str::<serde_json::Value>(tags).ok())
    else {
        return Evidence::None;
    };
    let mut artists = BTreeSet::new();
    for tag in tags["musicbrainz_artist_tags"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if tag["role"] != "artist" {
            continue;
        }
        let Some(value) = tag["value"].as_str() else {
            return Evidence::Ambiguous;
        };
        match Uuid::parse_str(value.trim()) {
            Ok(id)
                if provider::validate_builtin_id(&id.hyphenated().to_string()).is_ok()
                    && value.trim().len() == 36 =>
            {
                artists.insert(id.hyphenated().to_string());
            }
            _ => return Evidence::Ambiguous,
        }
    }
    let mut artists = artists.into_iter();
    match (artists.next(), artists.next()) {
        (None, _) => Evidence::None,
        (Some(artist), None) => Evidence::Artist(artist),
        _ => Evidence::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::{Evidence, evidence};

    fn tags(values: &[(&str, &str)]) -> String {
        let items: Vec<String> = values
            .iter()
            .map(|(role, value)| format!(r#"{{"role":"{role}","key":"k","value":"{value}","location":{{"kind":"format"}}}}"#))
            .collect();
        format!(r#"{{"musicbrainz_artist_tags":[{}]}}"#, items.join(","))
    }

    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn only_one_distinct_performer_identifier_is_evidence() {
        assert!(matches!(evidence(Some(&tags(&[("artist", A)]))), Evidence::Artist(id) if id == A));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", A), ("artist", &A.to_uppercase())]))),
            Evidence::Artist(id) if id == A
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", A), ("artist", B)]))),
            Evidence::Ambiguous
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", &format!("{A};{B}"))]))),
            Evidence::Ambiguous
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", "not-an-id")]))),
            Evidence::Ambiguous
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("album_artist", A)]))),
            Evidence::None
        ));
        assert!(matches!(evidence(None), Evidence::None));
    }
}
