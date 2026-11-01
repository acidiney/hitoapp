use super::RecordingGroupProof;
use crate::{
    Result,
    artist_catalog::{Title, tolerance_ms},
    store::Store,
};
use sqlx::SqliteConnection;
use std::{collections::BTreeSet, time::Instant};

const KEYS_PER_STEP: i64 = 400;
const MAX_COPIES: i64 = 12;

pub(crate) struct GroupStep {
    pub next: Option<String>,
    pub grouped: i64,
}

#[derive(Clone, Copy)]
enum Key {
    Recording,
    Isrc,
}

impl Key {
    fn tag(self) -> &'static str {
        match self {
            Self::Recording => "m",
            Self::Isrc => "i",
        }
    }

    fn proof(self) -> RecordingGroupProof {
        match self {
            Self::Recording => RecordingGroupProof::MusicbrainzRecording,
            Self::Isrc => RecordingGroupProof::Isrc,
        }
    }

    fn query(self) -> &'static str {
        match self {
            Self::Recording => {
                "SELECT mbid,recording_id,observation_id FROM recording_musicbrainz_observations
                 WHERE entity='recording' AND mbid>?1 ORDER BY mbid,recording_id LIMIT ?2"
            }
            Self::Isrc => {
                "SELECT isrc,recording_id,observation_id FROM recording_isrc_observations
                 WHERE isrc>?1 ORDER BY isrc,recording_id LIMIT ?2"
            }
        }
    }
}

fn position(cursor: Option<&str>) -> (Key, String) {
    match cursor.and_then(|cursor| cursor.split_once(':')) {
        Some(("i", key)) => (Key::Isrc, key.to_owned()),
        Some((_, key)) => (Key::Recording, key.to_owned()),
        None => (Key::Recording, String::new()),
    }
}

struct Candidate {
    id: String,
    title: String,
    duration_ms: i64,
    rank: u8,
    decisions: BTreeSet<&'static str>,
}

impl Candidate {
    fn same_version_and_length(&self, other: &Self) -> bool {
        if self.duration_ms <= 0 || other.duration_ms <= 0 {
            return false;
        }
        let (mine, theirs) = (Title::parse(&self.title), Title::parse(&other.title));
        mine == theirs
            && tolerance_ms(&mine, &theirs)
                .is_some_and(|tolerance| self.duration_ms.abs_diff(other.duration_ms) <= tolerance)
    }

    /// The card may show this recording instead of the other without losing playability or a
    /// personal choice.
    fn can_carry(&self, other: &Self) -> bool {
        self.rank >= other.rank && other.decisions.is_subset(&self.decisions)
    }
}

/// One bounded step of the account's grouping pass. Recordings are grouped only on proof: the same
/// tagged MusicBrainz recording or the same ISRC, with the same version wording and a compatible
/// length. The cursor continues the identifier walk.
pub(crate) async fn group_step(
    store: &Store,
    account: &str,
    cursor: Option<&str>,
) -> Result<GroupStep> {
    let started = Instant::now();
    let (key, after) = position(cursor);
    let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    let rows: Vec<(String, String, String)> = sqlx::query_as(key.query())
        .bind(&after)
        .bind(KEYS_PER_STEP)
        .fetch_all(&mut *tx)
        .await?;
    let full = rows.len() as i64 == KEYS_PER_STEP;
    let last = rows.last().map(|row| row.0.clone());
    let only_key = rows.iter().all(|row| Some(&row.0) == last.as_ref());
    let mut next = match key {
        Key::Recording => Some("i:".to_owned()),
        Key::Isrc => None,
    };
    let mut usable: &[(String, String, String)] = &rows;
    if full {
        let boundary = last.clone().unwrap_or_default();
        if only_key {
            next = Some(format!("{}:{boundary}", key.tag()));
        } else {
            let complete = rows.partition_point(|row| row.0 != boundary);
            usable = &rows[..complete];
            let previous = usable.last().map(|row| row.0.clone()).unwrap_or_default();
            next = Some(format!("{}:{previous}", key.tag()));
        }
    }
    let mut grouped = 0;
    let mut undecided = 0;
    let mut examined = 0;
    let mut start = 0;
    while start < usable.len() {
        let end = start
            + usable[start..]
                .iter()
                .take_while(|row| row.0 == usable[start].0)
                .count();
        if end - start > 1 {
            let mut ids = Vec::new();
            for (_, recording, observation) in &usable[start..end] {
                let visible: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM recordings r
                     JOIN library_members m ON m.library_id=r.library_id AND m.user_id=?1
                     WHERE r.id=?2 AND r.metadata_observation_id=?3)",
                )
                .bind(account)
                .bind(recording)
                .bind(observation)
                .fetch_one(&mut *tx)
                .await?;
                if visible {
                    ids.push(recording.as_str());
                }
            }
            if ids.len() > 1 {
                examined += 1;
                let (attached, left) = group_ids(&mut tx, account, key.proof(), &ids).await?;
                grouped += attached;
                undecided += left;
            }
        }
        start = end;
    }
    tx.commit().await?;
    tracing::info!(
        event = "recording_groups_step",
        proof = key.tag(),
        identifiers = examined,
        grouped,
        undecided,
        finished = next.is_none(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording grouping step finished"
    );
    Ok(GroupStep { next, grouped })
}

async fn group_ids(
    tx: &mut SqliteConnection,
    account: &str,
    proof: RecordingGroupProof,
    ids: &[&str],
) -> Result<(i64, i64)> {
    let mut free = Vec::new();
    let mut anchors = Vec::new();
    for id in ids {
        let canonical: Option<String> = sqlx::query_scalar(
            "SELECT canonical_id FROM recording_group_members WHERE user_id=? AND recording_id=?",
        )
        .bind(account)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let has_members: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM recording_group_members WHERE user_id=? AND canonical_id=?)",
        )
        .bind(account)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        match (canonical, has_members) {
            (Some(root), _) => {
                if !anchors.iter().any(|anchor: &Candidate| anchor.id == root) {
                    anchors.push(candidate(tx, account, &root).await?);
                }
            }
            (None, true) => {
                if !anchors.iter().any(|anchor: &Candidate| anchor.id == *id) {
                    anchors.push(candidate(tx, account, id).await?);
                }
            }
            (None, false) => free.push(candidate(tx, account, id).await?),
        }
    }
    let mut grouped = 0;
    for anchor in &anchors {
        let mut index = 0;
        while index < free.len() {
            if attachable(tx, account, anchor, &free[index]).await? {
                attach(tx, account, proof, &anchor.id, &free[index].id).await?;
                free.remove(index);
                grouped += 1;
            } else {
                index += 1;
            }
        }
    }
    free.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then(b.decisions.len().cmp(&a.decisions.len()))
            .then(a.id.cmp(&b.id))
    });
    while !free.is_empty() {
        let carrier = free.remove(0);
        let mut attached = false;
        let mut index = 0;
        while index < free.len() {
            if attachable(tx, account, &carrier, &free[index]).await? {
                attach(tx, account, proof, &carrier.id, &free[index].id).await?;
                free.remove(index);
                grouped += 1;
                attached = true;
            } else {
                index += 1;
            }
        }
        let _ = attached;
    }
    Ok((grouped, ids.len() as i64 - grouped))
}

async fn attachable(
    tx: &mut SqliteConnection,
    account: &str,
    carrier: &Candidate,
    other: &Candidate,
) -> Result<bool> {
    if !carrier.can_carry(other) || !carrier.same_version_and_length(other) {
        return Ok(false);
    }
    let separated: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM recording_group_splits s WHERE s.user_id=?1 AND
            ((s.recording_id=?2 AND s.other_id IN (SELECT ?3 UNION SELECT recording_id FROM recording_group_members WHERE user_id=?1 AND canonical_id=?3))
          OR (s.other_id=?2 AND s.recording_id IN (SELECT ?3 UNION SELECT recording_id FROM recording_group_members WHERE user_id=?1 AND canonical_id=?3))))",
    )
    .bind(account)
    .bind(&other.id)
    .bind(&carrier.id)
    .fetch_one(&mut *tx)
    .await?;
    if separated {
        return Ok(false);
    }
    let members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM recording_group_members WHERE user_id=? AND canonical_id=?",
    )
    .bind(account)
    .bind(&carrier.id)
    .fetch_one(&mut *tx)
    .await?;
    Ok(members + 1 < MAX_COPIES)
}

async fn attach(
    tx: &mut SqliteConnection,
    account: &str,
    proof: RecordingGroupProof,
    carrier: &str,
    other: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO recording_group_members(user_id,recording_id,canonical_id,proof) VALUES(?,?,?,?)",
    )
    .bind(account)
    .bind(other)
    .bind(carrier)
    .bind(proof.as_str())
    .execute(tx)
    .await?;
    Ok(())
}

async fn candidate(tx: &mut SqliteConnection, account: &str, id: &str) -> Result<Candidate> {
    let (title, duration_ms, rank): (String, i64, i64) = sqlx::query_as(
        "SELECT r.title,r.duration_ms,
            CASE WHEN EXISTS(SELECT 1 FROM sources s WHERE s.recording_id=r.id AND s.ready=1) THEN 2
                 WHEN EXISTS(SELECT 1 FROM provider_items item JOIN plugin_installations p ON p.id=item.installation_id
                    WHERE item.recording_id=r.id AND item.retired=0 AND p.user_id=?1) THEN 1 ELSE 0 END
         FROM recordings r WHERE r.id=?2",
    )
    .bind(account)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let decisions: Vec<String> = sqlx::query_scalar(
        "SELECT 'like' FROM likes WHERE user_id=?1 AND recording_id=?2 AND liked=1
         UNION ALL SELECT 'edit' FROM personal_recording_metadata WHERE user_id=?1 AND recording_id=?2
         UNION ALL SELECT 'source' FROM recording_source_choices WHERE user_id=?1 AND recording_id=?2",
    )
    .bind(account)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    Ok(Candidate {
        id: id.to_owned(),
        title,
        duration_ms,
        rank: rank as u8,
        decisions: decisions
            .iter()
            .map(|decision| match decision.as_str() {
                "like" => "like",
                "edit" => "edit",
                _ => "source",
            })
            .collect(),
    })
}
