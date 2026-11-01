use super::{DECISIONS, RecordingGroupProof, rank_sql};
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

enum Phase {
    Verify(String),
    Prove(Key, String),
}

fn position(cursor: Option<&str>) -> Phase {
    match cursor.and_then(|cursor| cursor.split_once(':')) {
        Some(("m", key)) => Phase::Prove(Key::Recording, key.to_owned()),
        Some(("i", key)) => Phase::Prove(Key::Isrc, key.to_owned()),
        Some((_, key)) => Phase::Verify(key.to_owned()),
        None => Phase::Verify(String::new()),
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
            && mine.same_specific_wording(&theirs)
            && tolerance_ms(&mine, &theirs)
                .is_some_and(|tolerance| self.duration_ms.abs_diff(other.duration_ms) <= tolerance)
    }

    fn can_carry(&self, other: &Self) -> bool {
        self.rank >= other.rank && other.decisions.is_subset(&self.decisions)
    }
}

pub(crate) async fn group_step(
    store: &Store,
    account: &str,
    cursor: Option<&str>,
) -> Result<GroupStep> {
    let started = Instant::now();
    let (key, after) = match position(cursor) {
        Phase::Verify(after) => return verify_step(store, account, &after, started).await,
        Phase::Prove(key, after) => (key, after),
    };
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
        let mut index = 0;
        while index < free.len() {
            if attachable(tx, account, &carrier, &free[index]).await? {
                attach(tx, account, proof, &carrier.id, &free[index].id).await?;
                free.remove(index);
                grouped += 1;
            } else {
                index += 1;
            }
        }
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
    let (title, duration_ms, rank): (String, i64, i64) = sqlx::query_as(&format!(
        "SELECT r.title,r.duration_ms,{} FROM recordings r WHERE r.id=?2",
        rank_sql("r.id", "?1")
    ))
    .bind(account)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let held = DECISIONS
        .iter()
        .map(|(table, column, filter)| {
            format!(
                "SELECT '{table}' FROM {table} held WHERE held.{column}=?1 AND held.recording_id=?2 {}",
                filter.replace("{}", "held")
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let held: Vec<String> = sqlx::query_scalar(&held)
        .bind(account)
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    Ok(Candidate {
        id: id.to_owned(),
        title,
        duration_ms,
        rank: rank as u8,
        decisions: DECISIONS
            .iter()
            .map(|decision| decision.0)
            .filter(|table| held.iter().any(|name| name == table))
            .collect(),
    })
}

const VERIFIED_PER_STEP: i64 = 200;

async fn verify_step(
    store: &Store,
    account: &str,
    after: &str,
    started: Instant,
) -> Result<GroupStep> {
    let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    let members: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT recording_id,canonical_id,proof FROM recording_group_members
         WHERE user_id=?1 AND recording_id>?2 ORDER BY recording_id LIMIT ?3",
    )
    .bind(account)
    .bind(after)
    .bind(VERIFIED_PER_STEP)
    .fetch_all(&mut *tx)
    .await?;
    let mut dropped = 0;
    for (member, carrier, proof) in &members {
        let holds = still_holds(&mut tx, account, member, carrier, proof).await?;
        if !holds {
            sqlx::query("DELETE FROM recording_group_members WHERE user_id=? AND recording_id=?")
                .bind(account)
                .bind(member)
                .execute(&mut *tx)
                .await?;
            dropped += 1;
        }
    }
    tx.commit().await?;
    let next = if members.len() as i64 == VERIFIED_PER_STEP {
        members.last().map(|member| format!("v:{}", member.0))
    } else {
        Some("m:".to_owned())
    };
    tracing::info!(
        event = "recording_groups_verified",
        checked = members.len(),
        dropped,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Recording group proofs checked"
    );
    Ok(GroupStep { next, grouped: 0 })
}

async fn still_holds(
    tx: &mut SqliteConnection,
    account: &str,
    member: &str,
    carrier: &str,
    proof: &str,
) -> Result<bool> {
    let shared: bool = match proof {
        "isrc" => {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM recording_isrc_observations a
                   JOIN recordings ra ON ra.id=a.recording_id AND ra.metadata_observation_id=a.observation_id
                   JOIN recording_isrc_observations b ON b.isrc=a.isrc
                   JOIN recordings rb ON rb.id=b.recording_id AND rb.metadata_observation_id=b.observation_id
                   WHERE ra.id=?2 AND rb.id<>ra.id AND (rb.id=?3 OR rb.id IN
                      (SELECT recording_id FROM recording_group_members WHERE user_id=?1 AND canonical_id=?3)))",
            )
        }
        _ => {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM recording_musicbrainz_observations a
                   JOIN recordings ra ON ra.id=a.recording_id AND ra.metadata_observation_id=a.observation_id
                   JOIN recording_musicbrainz_observations b ON b.entity='recording' AND b.mbid=a.mbid
                   JOIN recordings rb ON rb.id=b.recording_id AND rb.metadata_observation_id=b.observation_id
                   WHERE a.entity='recording' AND ra.id=?2 AND rb.id<>ra.id AND (rb.id=?3 OR rb.id IN
                      (SELECT recording_id FROM recording_group_members WHERE user_id=?1 AND canonical_id=?3)))",
            )
        }
    }
    .bind(account)
    .bind(member)
    .bind(carrier)
    .fetch_one(&mut *tx)
    .await?;
    if !shared {
        return Ok(false);
    }
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT title,duration_ms FROM recordings WHERE id IN (?1,?2) ORDER BY id=?2",
    )
    .bind(carrier)
    .bind(member)
    .fetch_all(&mut *tx)
    .await?;
    let [
        (carrier_title, carrier_length),
        (member_title, member_length),
    ] = rows.as_slice()
    else {
        return Ok(false);
    };
    let at = |title: &str, length: &i64| Candidate {
        id: String::new(),
        title: title.to_owned(),
        duration_ms: *length,
        rank: 0,
        decisions: BTreeSet::new(),
    };
    Ok(at(carrier_title, carrier_length).same_version_and_length(&at(member_title, member_length)))
}
