use super::{ArtistMatchReceipt, MatchArtistsFromTags};
use crate::catalog::artist_catalog::{
    ArtistCreditRole, AssociateRecordingArtist, MetadataProvider, REQUEST_BUDGET,
    association_changes, associations,
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

/// A choice written by an earlier automatic match that no decision of the account has replaced,
/// and whose observation is no longer current. Only such a choice may be matched again.
const REPLACEABLE: &str = "(EXISTS (SELECT 1 FROM artist_association_matches mt
        WHERE mt.association_id=c.association_id AND mt.receipt_order=c.selection_order)
    AND (c.retired=1 OR c.observation_id IS NOT r.metadata_observation_id))";

enum Evidence {
    None,
    Ambiguous,
    Artist(String),
}

struct Candidate {
    recording: String,
    evidence: Evidence,
    observation: String,
    operation: Option<String>,
}

struct Pending {
    recording: String,
    artist: String,
    association: String,
    created: bool,
    revision: i64,
    credit: Credit,
}

enum Planned {
    Match(Pending),
    Declined { remember: bool },
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
        let result = tokio::time::timeout(REQUEST_BUDGET, self.accept_tag_matches(actor, request))
            .await
            .unwrap_or(Err(Error::Conflict(
                "Artist matching took too long. Try again.".into(),
            )));
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
        let empty = |next_after| ArtistMatchReceipt {
            request_id: request.request_id,
            matched: 0,
            skipped_ambiguous: 0,
            skipped_declined: 0,
            next_after,
        };
        let (candidates, next_after) = {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
            let settings = policy::read(&mut connection, actor).await?;
            if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
                return Ok(empty(None));
            }
            candidates(&mut connection, actor, request.after.as_deref(), limit).await?
        };
        if candidates.is_empty() {
            return Ok(empty(next_after));
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Ok(empty(None));
        }
        let (mut ambiguous, mut declined) = (0, 0);
        let mut pending = Vec::new();
        for candidate in candidates {
            let remember = match candidate.evidence {
                Evidence::None => true,
                Evidence::Ambiguous => {
                    ambiguous += 1;
                    true
                }
                Evidence::Artist(ref artist) => {
                    match plan(
                        &mut tx,
                        actor,
                        &candidate.recording,
                        artist,
                        settings.revision,
                    )
                    .await?
                    {
                        Planned::Match(item) => {
                            pending.push(item);
                            false
                        }
                        Planned::Declined { remember } => {
                            declined += 1;
                            remember
                        }
                    }
                }
            };
            if remember {
                sqlx::query("INSERT INTO artist_match_checks(user_id,recording_id,observation_id,artist_operation_id) VALUES(?,?,?,?)
                    ON CONFLICT(user_id,recording_id) DO UPDATE SET observation_id=excluded.observation_id,artist_operation_id=excluded.artist_operation_id")
                    .bind(actor.id()).bind(&candidate.recording).bind(&candidate.observation).bind(&candidate.operation)
                    .execute(&mut *tx).await?;
            }
        }
        let mut receipt = empty(next_after);
        receipt.matched = pending.len() as i64;
        receipt.skipped_ambiguous = ambiguous;
        receipt.skipped_declined = declined;
        if !pending.is_empty() {
            let order =
                receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
            let (kind, installation) = provider_columns(&MetadataProvider::BuiltinMusicBrainz);
            for item in &pending {
                let group =
                    choices::ensure_group(&mut tx, actor, kind, installation, &item.artist).await?;
                choices::store(
                    &mut tx,
                    actor,
                    &item.recording,
                    item.revision,
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
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn candidates(
    connection: &mut SqliteConnection,
    actor: &Actor,
    after: Option<&str>,
    limit: i64,
) -> Result<(Vec<Candidate>, Option<String>)> {
    let mut rows: Vec<(String, Option<String>, String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT r.id,o.tags,r.metadata_observation_id,p.artist_operation_id FROM recordings r
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=?
         JOIN metadata_observations o ON o.id=r.metadata_observation_id AND o.recording_id=r.id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=m.user_id
         WHERE r.id>? AND o.source_id IS NOT NULL
            AND json_array_length(json_extract(o.tags,'$.musicbrainz_artist_tags'))>0
            AND NOT EXISTS (SELECT 1 FROM artist_match_checks k WHERE k.user_id=m.user_id
                AND k.recording_id=r.id AND k.observation_id=r.metadata_observation_id
                AND k.artist_operation_id IS p.artist_operation_id)
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=m.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})
         ORDER BY r.id LIMIT ?"
    ))
    .bind(actor.id())
    .bind(after.unwrap_or_default())
    .bind(limit + 1)
    .fetch_all(connection)
    .await?;
    let next_after = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        rows.last().map(|row| row.0.clone())
    } else {
        None
    };
    let candidates = rows
        .into_iter()
        .map(|(recording, tags, observation, operation)| Candidate {
            recording,
            evidence: evidence(tags.as_deref()),
            observation,
            operation,
        })
        .collect();
    Ok((candidates, next_after))
}

async fn plan(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    artist: &str,
    policy_revision: i64,
) -> Result<Planned> {
    let choice: Option<(i64, bool)> = sqlx::query_as(&format!(
        "SELECT c.revision,{REPLACEABLE} FROM recording_artist_identity_choices c
         JOIN recordings r ON r.id=c.recording_id WHERE c.user_id=? AND c.recording_id=?"
    ))
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(&mut *tx)
    .await?;
    let revision = match choice {
        Some((_, false)) => return Ok(Planned::Declined { remember: false }),
        Some((revision, true)) => revision,
        None => 0,
    };
    let Some(revision) = revision.checked_add(1) else {
        return Ok(Planned::Declined { remember: false });
    };
    let credit = match Credit::capture(tx, actor, recording).await {
        Ok(credit) if credit.credit_operation.is_none() => credit,
        Ok(_) => return Ok(Planned::Declined { remember: true }),
        Err(Error::NotFound) => return Ok(Planned::Declined { remember: false }),
        Err(error) => return Err(error),
    };
    let hidden: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM shared_artist_correction_subjects subject
            WHERE subject.retired_reason IS NULL AND subject.recording_id=? AND subject.observation_id=?
                AND subject.provider_artist_id=?
                AND (SELECT state.applied FROM shared_artist_correction_states state
                    WHERE state.correction_seq=subject.correction_seq ORDER BY state.revision DESC LIMIT 1)=1)",
    )
    .bind(recording)
    .bind(&credit.observation_id)
    .bind(artist)
    .fetch_one(&mut *tx)
    .await?;
    if hidden {
        return Ok(Planned::Declined { remember: true });
    }
    let request = AssociateRecordingArtist {
        artist_id: credit.artist_id.clone(),
        source_id: credit.source_id.clone(),
        observation_id: credit.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: MetadataProvider::BuiltinMusicBrainz,
        provider_artist_id: artist.into(),
        policy_revision,
    };
    let existing = associations::existing(tx, actor, recording, &request).await?;
    if let Some(id) = &existing
        && !association_changes::is_linked(tx, id).await?
    {
        return Ok(Planned::Declined { remember: true });
    }
    let mut attempt = tx.begin().await?;
    match associations::retain(&mut attempt, actor, recording, &credit.library_id, &request).await {
        Ok(association) => {
            attempt.commit().await?;
            Ok(Planned::Match(Pending {
                recording: recording.into(),
                artist: artist.into(),
                association: association.id,
                created: existing.is_none(),
                revision,
                credit,
            }))
        }
        Err(Error::Conflict(_) | Error::NotFound) => {
            attempt.rollback().await?;
            Ok(Planned::Declined { remember: true })
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
