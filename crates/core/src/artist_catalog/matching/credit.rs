//! Credit-level matching: once individual tracks of one credit are matched to exactly one
//! MusicBrainz artist, the credit's other undecided tracks that carry no evidence of their own
//! receive the same artist, so one identified track does not leave its 100 siblings separate.
use super::apply::{EvidenceKind, Origin, Pending, Planned, REPLACEABLE, plan};
use crate::artist_catalog::identity::groups::current_choices;
use crate::{Actor, Result};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use std::collections::BTreeMap;

/// Tracks of one credit that may follow in a single settlement.
const MAX_SIBLINGS: i64 = 1000;

/// Folds the compatibility forms of the separators Hito looks for (full-width punctuation and
/// the ideographic comma) to ASCII and lower-cases the rest.
fn folded(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '\u{ff01}'..='\u{ff5e}' => {
                char::from_u32(u32::from(character) - 0xfee0).unwrap_or(character)
            }
            '\u{3000}' => ' ',
            '\u{3001}' => ',',
            other => other,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

const SEPARATOR_WORDS: [&str; 13] = [
    "and",
    "und",
    "et",
    "y",
    "with",
    "vs",
    "x",
    "feat",
    "ft",
    "featuring",
    "presents",
    "pres",
    "meets",
];

fn lists_several(text: &str) -> bool {
    text.contains(['&', ',', ';', '/', '+', '|', '\u{d7}'])
        || text
            .split(|character: char| !character.is_alphanumeric())
            .any(|word| SEPARATOR_WORDS.contains(&word))
}

/// A credit name that lists several artists names no single one. Hito does not split credits, so
/// such a credit receives nothing beyond the tracks that were matched individually. Bracketed
/// parts are checked on their own ("(feat. B)") and then ignored ("(Live)").
fn names_several(name: &str) -> bool {
    let name = folded(name);
    let (mut outside, mut inside, mut depth) = (String::new(), String::new(), 0_u32);
    for character in name.chars() {
        match character {
            '(' | '[' | '{' => {
                depth += 1;
                inside.push(' ');
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                inside.push(' ');
            }
            other if depth > 0 => inside.push(other),
            other => outside.push(other),
        }
    }
    lists_several(&outside) || lists_several(&inside)
}

struct Group {
    /// How many of the credit's tracks name each artist by strong evidence (file tag identifier,
    /// ISRC or provider artist page), matched now or earlier, with the evidence of the first.
    /// Personal choices and name matches do not vote, and each recording counts once.
    artists: BTreeMap<String, (u32, Option<Origin>)>,
}

impl Group {
    /// The one artist every strong match of the credit names, or `None` when they disagree.
    fn unanimous(&self) -> Option<(&str, Option<Origin>, u32)> {
        let mut artists = self.artists.iter();
        let (artist, (count, origin)) = artists.next()?;
        artists
            .next()
            .is_none()
            .then_some((artist.as_str(), *origin, *count))
    }
}

fn origin_named(name: &str) -> Option<Origin> {
    match name {
        "file_tag_artist_id" => Some(Origin::FileTagArtistId),
        "isrc" => Some(Origin::Isrc),
        "provider_url" => Some(Origin::ProviderUrl),
        "name_discography" => Some(Origin::NameDiscography),
        _ => None,
    }
}

/// The credits a settlement touched: those of its matches and of every recording it examined.
async fn touched(
    tx: &mut SqliteConnection,
    actor: &Actor,
    pending: &[Pending],
    examined: &[String],
) -> Result<BTreeMap<String, String>> {
    let mut credits: BTreeMap<String, String> = pending
        .iter()
        .map(|item| {
            (
                item.credit_artist().to_owned(),
                item.credit_name().to_owned(),
            )
        })
        .collect();
    let listed = serde_json::to_string(examined).unwrap_or_default();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT a.id,a.name FROM account_recording_credits rc
         JOIN account_artists a ON a.user_id=rc.user_id AND a.id=rc.artist_id
         WHERE rc.user_id=? AND rc.recording_id IN (SELECT value FROM json_each(?))",
    )
    .bind(actor.id())
    .bind(listed)
    .fetch_all(&mut *tx)
    .await?;
    credits.extend(rows);
    Ok(credits)
}

/// How many of the credit's tracks currently use each artist because of strong automatic
/// evidence, leaving out recordings that this settlement matches again.
async fn strong_votes(
    tx: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    matching_now: &str,
) -> Result<Vec<(String, i64)>> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT (SELECT g.provider_artist_id FROM account_artist_identity_groups g
            WHERE g.user_id=choice.user_id AND g.id=choice.group_id) AS identity,count(*) ",
    );
    query.push(current_choices());
    query
        .push_bind(actor.id().to_owned())
        .push(" AND choice.artist_id=")
        .push_bind(credit.to_owned())
        .push(
            " AND EXISTS (SELECT 1 FROM (
                SELECT association_id,receipt_order FROM artist_association_matches
                UNION ALL SELECT association_id,receipt_order FROM artist_association_provider_matches) mt
              WHERE mt.association_id=choice.association_id AND mt.receipt_order=choice.selection_order)
             AND choice.recording_id NOT IN (SELECT value FROM json_each(",
        )
        .push_bind(matching_now.to_owned())
        .push(")) GROUP BY identity");
    Ok(query.build_query_as().fetch_all(tx).await?)
}

/// The evidence an earlier match of the credit's tracks to this artist was made from.
async fn earlier_origin(
    tx: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    artist: &str,
) -> Result<Option<Origin>> {
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT m.evidence_kind FROM recording_artist_identity_choices c
         JOIN recording_artist_associations a ON a.id=c.association_id
         JOIN (SELECT association_id,evidence_kind FROM artist_association_matches
               UNION ALL SELECT association_id,evidence_kind FROM artist_association_provider_matches) m
            ON m.association_id=c.association_id
         WHERE c.user_id=? AND c.artist_id=? AND a.provider_artist_id=? AND c.retired=0 LIMIT 1",
    )
    .bind(actor.id())
    .bind(credit)
    .bind(artist)
    .fetch_optional(tx)
    .await?;
    Ok(kind.as_deref().and_then(origin_named))
}

/// Whether a track of the credit still has evidence of its own that no pass has examined, so the
/// credit's answer is not complete yet. Recordings in `examined` are being settled now.
async fn evidence_unexamined(
    tx: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    examined: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM account_recording_credits rc
         JOIN recordings r ON r.id=rc.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
         JOIN metadata_observations o ON o.id=r.metadata_observation_id AND o.recording_id=r.id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=rc.user_id
         WHERE rc.user_id=? AND rc.artist_id=? AND r.id NOT IN (SELECT value FROM json_each(?))
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=rc.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})
            AND ((EXISTS (SELECT 1 FROM recording_isrc_observations i WHERE i.observation_id=o.id)
                    OR (SELECT count(*) FROM recording_artist_url_observations u WHERE u.observation_id=o.id)=1)
                AND NOT EXISTS (SELECT 1 FROM artist_provider_checks k WHERE k.user_id=rc.user_id
                    AND k.recording_id=r.id AND k.observation_id=o.id AND k.artist_operation_id IS p.artist_operation_id)
              OR COALESCE(json_array_length(json_extract(o.tags,'$.musicbrainz_artist_tags')),0)>0
                AND NOT EXISTS (SELECT 1 FROM artist_match_checks k WHERE k.user_id=rc.user_id
                    AND k.recording_id=r.id AND k.observation_id=o.id AND k.artist_operation_id IS p.artist_operation_id)))"
    ))
    .bind(actor.id())
    .bind(credit)
    .bind(examined)
    .fetch_one(tx)
    .await?)
}

/// Whether a track of the credit with strong evidence of its own was examined without a clear
/// answer (or could not be asked), so the credit's evidence is not unanimous.
async fn evidence_unsettled(
    tx: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    unsettled: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM account_recording_credits rc
         JOIN recordings r ON r.id=rc.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
         JOIN metadata_observations o ON o.id=r.metadata_observation_id AND o.recording_id=r.id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=rc.user_id
         WHERE rc.user_id=? AND rc.artist_id=?
            AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                WHERE c.user_id=rc.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})
            AND (EXISTS (SELECT 1 FROM recording_isrc_observations i WHERE i.observation_id=o.id)
                OR (SELECT count(*) FROM recording_artist_url_observations u WHERE u.observation_id=o.id)=1
                OR COALESCE(json_array_length(json_extract(o.tags,'$.musicbrainz_artist_tags')),0)>0)
            AND (r.id IN (SELECT value FROM json_each(?))
                OR EXISTS (SELECT 1 FROM artist_provider_checks k WHERE k.user_id=rc.user_id
                    AND k.recording_id=r.id AND k.observation_id=o.id AND k.artist_operation_id IS p.artist_operation_id)
                OR EXISTS (SELECT 1 FROM artist_match_checks k WHERE k.user_id=rc.user_id
                    AND k.recording_id=r.id AND k.observation_id=o.id AND k.artist_operation_id IS p.artist_operation_id)))"
    ))
    .bind(actor.id())
    .bind(credit)
    .bind(unsettled)
    .fetch_one(tx)
    .await?)
}

/// Extends the matches of a settlement over the undecided tracks of the credits it touched and
/// returns the recordings that were added. A credit follows its artist only when every track with
/// evidence of its own has been examined and the credit's matched tracks name one artist more
/// often than all others together; a credit whose name lists several artists follows nothing.
/// Tracks with evidence of their own are never followed: their own answer decides them.
pub(super) async fn extend(
    tx: &mut SqliteConnection,
    actor: &Actor,
    pending: &mut Vec<Pending>,
    examined: &[String],
    unsettled: &[String],
    policy_revision: i64,
) -> Result<Vec<String>> {
    let credits = touched(tx, actor, pending, examined).await?;
    let examined = serde_json::to_string(examined).unwrap_or_default();
    let unsettled = serde_json::to_string(unsettled).unwrap_or_default();
    let matching_now = serde_json::to_string(
        &pending
            .iter()
            .map(|item| item.recording.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    let mut added = Vec::new();
    for (credit, name) in credits {
        if names_several(&name)
            || evidence_unexamined(tx, actor, &credit, &examined).await?
            || evidence_unsettled(tx, actor, &credit, &unsettled).await?
        {
            continue;
        }
        let mut group = Group {
            artists: BTreeMap::new(),
        };
        for (artist, count) in strong_votes(tx, actor, &credit, &matching_now).await? {
            group.artists.entry(artist).or_insert((0, None)).0 += count as u32;
        }
        for item in pending.iter().filter(|item| item.credit_artist() == credit) {
            let Some(origin) = item.kind.origin().map(Some) else {
                continue;
            };
            let entry = group
                .artists
                .entry(item.artist.clone())
                .or_insert((0, origin));
            entry.0 += 1;
            entry.1 = entry.1.or(origin);
        }
        let Some((artist, origin, tracks)) = group.unanimous() else {
            continue;
        };
        let artist = artist.to_owned();
        let origin = match origin {
            Some(origin) => origin,
            None => match earlier_origin(tx, actor, &credit, &artist).await? {
                Some(origin) => origin,
                None => continue,
            },
        };
        let siblings: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT r.id FROM account_recording_credits rc
             JOIN recordings r ON r.id=rc.recording_id
             JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
             JOIN metadata_observations o ON o.id=r.metadata_observation_id AND o.recording_id=r.id
             WHERE rc.user_id=? AND rc.artist_id=?
                AND NOT EXISTS (SELECT 1 FROM recording_isrc_observations i WHERE i.observation_id=o.id)
                AND NOT EXISTS (SELECT 1 FROM recording_artist_url_observations u WHERE u.observation_id=o.id)
                AND COALESCE(json_array_length(json_extract(o.tags,'$.musicbrainz_artist_tags')),0)=0
                AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
                    WHERE c.user_id=rc.user_id AND c.recording_id=r.id AND NOT {REPLACEABLE})
             ORDER BY r.id LIMIT ?"
        ))
        .bind(actor.id())
        .bind(&credit)
        .bind(MAX_SIBLINGS)
        .fetch_all(&mut *tx)
        .await?;
        for recording in siblings {
            if pending.iter().any(|item| item.recording == recording) {
                continue;
            }
            let kind = EvidenceKind::Credit(origin, tracks);
            if let Planned::Match(item) =
                plan(tx, actor, &recording, &artist, policy_revision, kind).await?
            {
                pending.push(*item);
                added.push(recording);
            }
        }
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::names_several;

    #[test]
    fn credits_listing_several_artists_are_not_one_artist() {
        for name in [
            "Ed Sheeran & Justin Bieber",
            "A \u{ff06} B",
            "A, B",
            "A\u{ff0c}B",
            "A \u{d7} B",
            "A x B",
            "A/B",
            "A + B",
            "A\u{3001}B",
            "A | B",
            "A and B",
            "A und B",
            "A et B",
            "A y B",
            "A with B",
            "A vs B",
            "A vs. B",
            "A feat. B",
            "A ft. B",
            "A featuring B",
            "A (feat. B)",
            "A(ft. B)",
            "A feat.B",
            "A [featuring B]",
            "A presents B",
            "A pres. B",
            "A meets B",
            "A; B",
        ] {
            assert!(names_several(name), "{name}");
        }
        for name in [
            "Ed Sheeran",
            "Nova Quartet",
            "Alexander",
            "Maxwell",
            "VSOP",
            "Wu-Tang Clan",
            "Yo La Tengo",
            "Xavier",
            "Beyonc\u{e9}",
            "Ed Sheeran (Live)",
            "Ed Sheeran [Remastered]",
            "Pet Shop Boys",
        ] {
            assert!(!names_several(name), "{name}");
        }
    }
}
