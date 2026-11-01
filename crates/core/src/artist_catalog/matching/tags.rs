use super::{
    ArtistMatchReceipt, MatchArtistsFromTags,
    apply::{self, EvidenceKind, Planned, REPLACEABLE, plan},
};
use crate::artist_catalog::{
    MetadataProvider, REQUEST_BUDGET, identity::receipts, policy, provider,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use sqlx::SqliteConnection;
use std::{collections::BTreeSet, time::Instant};
use uuid::Uuid;

const MAX_BATCH: i64 = 100;
const MAX_ID_LENGTH: usize = 64;

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

impl ArtistCatalog {
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
                        EvidenceKind::FileTagArtistId,
                    )
                    .await?
                    {
                        Planned::Match(item) => {
                            pending.push(*item);
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
            apply::commit(&mut tx, actor, &pending, order).await?;
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

type Row = (
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

async fn candidates(
    connection: &mut SqliteConnection,
    actor: &Actor,
    after: Option<&str>,
    limit: i64,
) -> Result<(Vec<Candidate>, Option<String>)> {
    let mut rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT r.id,o.tags,r.metadata_observation_id,p.artist_operation_id,
            (SELECT json_group_array(json_array(c.role,c.provider_artist_id)) FROM
                (SELECT role,provider_artist_id FROM recording_credited_artist_observations
                 WHERE observation_id=r.metadata_observation_id AND recording_id=r.id ORDER BY position) c)
         FROM recordings r
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
        .map(
            |(recording, tags, observation, operation, credits)| Candidate {
                recording,
                evidence: evidence(tags.as_deref(), credits.as_deref()),
                observation,
                operation,
            },
        )
        .collect();
    Ok((candidates, next_after))
}

fn artist_id(value: &str) -> Option<String> {
    let id = Uuid::parse_str(value.trim()).ok()?;
    (provider::validate_builtin_id(&id.hyphenated().to_string()).is_ok()
        && value.trim().len() == 36)
        .then(|| id.hyphenated().to_string())
}

fn credited_evidence(credits: Option<&str>) -> Option<Evidence> {
    let credits: Vec<serde_json::Value> = serde_json::from_str(credits?).ok()?;
    let mains: Vec<&serde_json::Value> = credits
        .iter()
        .filter(|credit| credit[0] == "main")
        .collect();
    match mains.as_slice() {
        [] => None,
        [only] => only[1].as_str().and_then(artist_id).map(Evidence::Artist),
        _ => Some(Evidence::Ambiguous),
    }
}

fn evidence(tags: Option<&str>, credits: Option<&str>) -> Evidence {
    if let Some(decided) = credited_evidence(credits) {
        return decided;
    }
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
        match artist_id(value) {
            Some(id) => {
                artists.insert(id);
            }
            None => return Evidence::Ambiguous,
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
        assert!(
            matches!(evidence(Some(&tags(&[("artist", A)])), None), Evidence::Artist(id) if id == A)
        );
        assert!(matches!(
            evidence(Some(&tags(&[("artist", A), ("artist", &A.to_uppercase())])), None),
            Evidence::Artist(id) if id == A
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", A), ("artist", B)])), None),
            Evidence::Ambiguous
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", &format!("{A};{B}"))])), None),
            Evidence::Ambiguous
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("artist", "not-an-id")])), None),
            Evidence::Ambiguous
        ));
        assert!(matches!(
            evidence(Some(&tags(&[("album_artist", A)])), None),
            Evidence::None
        ));
        assert!(matches!(evidence(None, None), Evidence::None));
    }

    #[test]
    fn listed_main_artists_decide_before_the_identifier_tags() {
        let both = tags(&[("artist", A), ("artist", B)]);
        let lone = format!(r#"[["main","{A}"],["featured","{B}"]]"#);
        assert!(matches!(evidence(Some(&both), Some(&lone)), Evidence::Artist(id) if id == A));
        let pair = format!(r#"[["main","{A}"],["main","{B}"]]"#);
        let one_tag = tags(&[("artist", A)]);
        assert!(matches!(
            evidence(Some(&one_tag), Some(&pair)),
            Evidence::Ambiguous
        ));
        let unnamed = r#"[["main",null],["featured",null]]"#;
        assert!(matches!(evidence(Some(&one_tag), Some(unnamed)), Evidence::Artist(id) if id == A));
        assert!(matches!(
            evidence(Some(&both), Some("[]")),
            Evidence::Ambiguous
        ));
    }
}
