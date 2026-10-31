//! Credit-level matching: once individual tracks of one credit are matched to exactly one
//! MusicBrainz artist, the credit's other undecided tracks that carry no evidence of their own
//! receive the same artist, so one identified track does not leave its 100 siblings separate.
use super::apply::{EvidenceKind, Origin, Pending, Planned, REPLACEABLE, plan};
use crate::{Actor, Result};
use sqlx::SqliteConnection;
use std::collections::BTreeMap;

/// Tracks of one credit that may follow in a single settlement.
const MAX_SIBLINGS: i64 = 1000;

/// A credit name that lists several artists names no single one. Hito does not split credits, so
/// such a credit receives nothing beyond the tracks that were matched individually.
fn names_several(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains(['&', ',', ';', '/', '+'])
        || [
            " feat", " ft.", " ft ", " and ", " with ", " x ", " vs", " med ", " con ",
        ]
        .iter()
        .any(|separator| name.contains(separator))
}

struct Group {
    /// How many individual matches each artist has in this credit, with the evidence of the first.
    artists: BTreeMap<String, (u32, Origin)>,
    name: String,
}

impl Group {
    /// The artist that more of the credit's matched tracks name than all others together.
    fn dominant(&self) -> Option<(&str, Origin, u32)> {
        let total: u32 = self.artists.values().map(|(count, _)| count).sum();
        self.artists
            .iter()
            .find(|(_, (count, _))| count * 2 > total)
            .map(|(artist, (count, origin))| (artist.as_str(), *origin, *count))
    }
}

/// Extends the individual matches of `pending` over the undecided tracks of the same credits and
/// returns the recordings that were added. A credit whose individually matched tracks name no
/// clear majority artist, or whose name lists several artists, is left as it is. Tracks with
/// evidence of their own are never followed: their own answer decides them.
pub(super) async fn extend(
    tx: &mut SqliteConnection,
    actor: &Actor,
    pending: &mut Vec<Pending>,
    policy_revision: i64,
) -> Result<Vec<String>> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for item in pending.iter() {
        let Some(origin) = item.kind.origin() else {
            continue;
        };
        let group = groups
            .entry(item.credit_artist().to_owned())
            .or_insert_with(|| Group {
                artists: BTreeMap::new(),
                name: item.credit_name().to_owned(),
            });
        group
            .artists
            .entry(item.artist.clone())
            .or_insert((0, origin))
            .0 += 1;
    }
    let mut added = Vec::new();
    for (credit, group) in groups {
        let Some((artist, origin, tracks)) = group.dominant() else {
            continue;
        };
        if names_several(&group.name) {
            continue;
        }
        let artist = artist.to_owned();
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
            "A, B",
            "A feat. B",
            "A x B",
            "A and B",
            "A/B",
        ] {
            assert!(names_several(name), "{name}");
        }
        for name in [
            "Ed Sheeran",
            "Nova Quartet",
            "Alexander",
            "Maxwell",
            "Wu-Tang Clan",
        ] {
            assert!(!names_several(name), "{name}");
        }
    }
}
