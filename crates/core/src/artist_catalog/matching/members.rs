use super::{
    ArtistProviderMatchReceipt,
    apply::AUTOMATIC_CHOICE,
    credit::{CreditPart, split_names},
    name_probe::normalize,
    settle::empty_receipt,
    tags::artist_id,
};
use crate::artist_catalog::{
    ArtistCreditRole, AssociateRecordingArtist, MetadataProvider, association_changes,
    associations,
    authority::{Credit, provider_columns},
    credit_target::CreditTarget,
    identity::{
        choices,
        groups::{current_choices, current_choices_joined},
        receipts,
    },
    policy,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::CreditRole;
use sqlx::{Connection, Sqlite, SqliteConnection, Transaction};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub(super) const MEMBER_CURSOR: &str = "member:";
const SPLIT_STAGE: &str = "1:";
const MEMBER_STAGE: &str = "2:";
const MAX_CREDITS: i64 = 50;
const MAX_RECORDINGS: i64 = 100;

pub(super) const MEMBER_REPLACEABLE: &str =
    "(EXISTS (SELECT 1 FROM artist_association_member_matches mt
        WHERE mt.association_id=c.association_id AND mt.receipt_order=c.selection_order)
    AND (c.retired=1 OR c.observation_id IS NOT r.metadata_observation_id))";

const AUTOMATIC_MEMBER_CHOICE: &str = "EXISTS (SELECT 1 FROM artist_association_member_matches mt
    WHERE mt.association_id=c.association_id AND mt.receipt_order=c.selection_order)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemberEvidence {
    SoloCredit,
    ListedArtistId,
    FileTagArtistId,
}

impl MemberEvidence {
    fn name(self) -> &'static str {
        match self {
            Self::SoloCredit => "solo_credit",
            Self::ListedArtistId => "listed_artist_id",
            Self::FileTagArtistId => "file_tag_artist_id",
        }
    }
}

struct Pending {
    recording: String,
    artist: String,
    role: CreditRole,
    revision: i64,
    association: String,
    provider_artist_id: String,
    observation: String,
    reused_order: Option<i64>,
    evidence: MemberEvidence,
}

struct Member {
    artist: String,
    name: String,
    role: CreditRole,
    listed: Option<String>,
    open: Option<i64>,
    decided: Option<String>,
}

#[derive(Default)]
struct Solos {
    known: HashMap<String, Option<String>>,
}

impl Solos {
    async fn identity(
        &mut self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        name: &str,
    ) -> Result<Option<String>> {
        let key = normalize(name);
        if let Some(known) = self.known.get(&key) {
            return Ok(known.clone());
        }
        let found = solo_identity(connection, actor, name).await?;
        self.known.insert(key, found.clone());
        Ok(found)
    }
}

fn probes(name: &str) -> Vec<String> {
    let mut probes = vec![
        name.trim().to_owned(),
        name.split_whitespace().collect::<Vec<_>>().join(" "),
        normalize(name),
    ];
    probes.sort();
    probes.dedup();
    probes.retain(|probe| !probe.is_empty());
    probes
}

async fn solo_identity(
    connection: &mut SqliteConnection,
    actor: &Actor,
    name: &str,
) -> Result<Option<String>> {
    let key = normalize(name);
    let mut credits = Vec::new();
    for probe in probes(name) {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT a.id,a.name FROM account_artists a
             JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
             WHERE a.user_id=? AND a.name=? COLLATE NOCASE",
        )
        .bind(actor.id())
        .bind(&probe)
        .fetch_all(&mut *connection)
        .await?;
        credits.extend(
            rows.into_iter()
                .filter(|(_, found)| normalize(found) == key)
                .map(|(id, _)| id),
        );
    }
    credits.sort();
    credits.dedup();
    if credits.is_empty() {
        return Ok(None);
    }
    let groups: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT DISTINCT g.provider_kind,g.provider_artist_id {} ? AND choice.artist_id IN (SELECT value FROM json_each(?)) LIMIT 2",
        current_choices_joined(
            "JOIN account_artist_identity_groups g ON g.user_id=choice.user_id AND g.id=choice.group_id"
        )
    ))
    .bind(actor.id())
    .bind(serde_json::to_string(&credits).map_err(|_| Error::Internal("Solo credits".into()))?)
    .fetch_all(connection)
    .await?;
    Ok(match groups.as_slice() {
        [(kind, id)] if kind == "builtin_musicbrainz" => artist_id(id),
        _ => None,
    })
}

fn role_of(role: &str) -> Option<CreditRole> {
    match role {
        "main" => Some(CreditRole::Main),
        "featured" => Some(CreditRole::Featured),
        _ => None,
    }
}

fn role_name(role: CreditRole) -> &'static str {
    match role {
        CreditRole::Main => "main",
        CreditRole::Featured => "featured",
    }
}

fn tagged_ids(tags: Option<&str>) -> Option<BTreeSet<String>> {
    let tags: serde_json::Value = serde_json::from_str(tags?).ok()?;
    let mut ids = BTreeSet::new();
    for tag in tags["musicbrainz_artist_tags"].as_array()? {
        if tag["role"] != "artist" {
            continue;
        }
        for value in tag["value"].as_str()?.split(';') {
            ids.insert(artist_id(value)?);
        }
    }
    Some(ids)
}

impl ArtistCatalog {
    pub(super) async fn match_members(
        &self,
        actor: &Actor,
        request_id: Uuid,
        digest: &str,
        after: &str,
    ) -> Result<ArtistProviderMatchReceipt> {
        let mut solos = Solos::default();
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            for name in names_to_vouch_for(&mut connection, actor, after).await? {
                solos.identity(&mut connection, actor, &name).await?;
            }
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request_id, digest).await? {
            return Ok(receipt);
        }
        let settings = policy::read(&mut tx, actor).await?;
        if !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz) {
            return Ok(empty_receipt(request_id, None));
        }
        let (pending, next_after) = match after.strip_prefix(MEMBER_STAGE) {
            Some(after) => {
                let (recordings, next) = unresolved(&mut tx, actor, after).await?;
                let mut pending = Vec::new();
                for recording in &recordings {
                    pending.extend(
                        plan(&mut tx, actor, recording, settings.revision, &mut solos).await?,
                    );
                }
                tracing::debug!(
                    recordings = recordings.len(),
                    planned = pending.len(),
                    "Credit members examined"
                );
                (
                    pending,
                    next.map(|recording| format!("{MEMBER_CURSOR}{MEMBER_STAGE}{recording}")),
                )
            }
            None => {
                let after = after.strip_prefix(SPLIT_STAGE).unwrap_or_default();
                let (credits, more) = split_candidates(&mut tx, actor, after).await?;
                for (credit, name) in &credits {
                    reconsider_split(&mut tx, actor, credit, name, &mut solos).await?;
                }
                let next = match credits.last() {
                    Some((last, _)) if more => format!("{MEMBER_CURSOR}{SPLIT_STAGE}{last}"),
                    _ => format!("{MEMBER_CURSOR}{MEMBER_STAGE}"),
                };
                (Vec::new(), Some(next))
            }
        };
        let mut receipt = empty_receipt(request_id, next_after);
        receipt.matched = pending.len() as i64;
        if !pending.is_empty() {
            let order = receipts::save(&mut tx, actor, digest, request_id, &receipt).await?;
            commit(&mut tx, actor, &pending, order).await?;
        }
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn names_to_vouch_for(
    connection: &mut SqliteConnection,
    actor: &Actor,
    after: &str,
) -> Result<Vec<String>> {
    if let Some(after) = after.strip_prefix(MEMBER_STAGE) {
        let (recordings, _) = unresolved(connection, actor, after).await?;
        return Ok(sqlx::query_scalar(
            "SELECT DISTINCT artist.name FROM account_recording_credit_members member
             JOIN account_artists artist ON artist.user_id=member.user_id AND artist.id=member.artist_id
             WHERE member.user_id=? AND member.recording_id IN (SELECT value FROM json_each(?))",
        )
        .bind(actor.id())
        .bind(serde_json::to_string(&recordings).map_err(|_| Error::Internal("Member recordings".into()))?)
        .fetch_all(connection)
        .await?);
    }
    let after = after.strip_prefix(SPLIT_STAGE).unwrap_or_default();
    let (credits, _) = split_candidates(connection, actor, after).await?;
    Ok(credits
        .iter()
        .filter_map(|(_, name)| split_names(name))
        .flatten()
        .map(|part| part.name)
        .collect())
}

async fn split_candidates(
    connection: &mut SqliteConnection,
    actor: &Actor,
    after: &str,
) -> Result<(Vec<(String, String)>, bool)> {
    let mut rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.id,a.name FROM (
            SELECT artist_id FROM artist_name_misses WHERE user_id=?1 AND artist_id>?2
            UNION SELECT artist_id FROM account_credit_splits WHERE user_id=?1 AND artist_id>?2) candidate
         JOIN account_artists a ON a.user_id=?1 AND a.id=candidate.artist_id
         JOIN library_members m ON m.library_id=a.library_id AND m.user_id=a.user_id
         ORDER BY a.id LIMIT ?3",
    )
    .bind(actor.id())
    .bind(after)
    .bind(MAX_CREDITS + 1)
    .fetch_all(connection)
    .await?;
    let more = rows.len() as i64 > MAX_CREDITS;
    rows.truncate(MAX_CREDITS as usize);
    Ok((rows, more))
}

async fn wanted_split(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    name: &str,
    solos: &mut Solos,
) -> Result<Option<Vec<CreditPart>>> {
    let (missed, whole, cleared): (bool, bool, bool) = sqlx::query_as(&format!(
        "SELECT EXISTS (SELECT 1 FROM artist_name_misses WHERE user_id=?1 AND artist_id=?2),
            EXISTS (SELECT 1 {} ?1 AND choice.artist_id=?2),
            EXISTS (SELECT 1 FROM recording_artist_identity_choices choice
                JOIN account_recording_credits whole ON whole.user_id=choice.user_id
                    AND whole.recording_id=choice.recording_id
                WHERE choice.user_id=?1 AND whole.artist_id=?2 AND NOT {AUTOMATIC_CHOICE})",
        current_choices()
    ))
    .bind(actor.id())
    .bind(credit)
    .fetch_one(&mut *connection)
    .await?;
    if !missed || whole || cleared {
        return Ok(None);
    }
    let Some(parts) = split_names(name) else {
        return Ok(None);
    };
    for part in &parts {
        if solos
            .identity(connection, actor, &part.name)
            .await?
            .is_none()
        {
            return Ok(None);
        }
    }
    Ok(Some(parts))
}

async fn reconsider_split(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    credit: &str,
    name: &str,
    solos: &mut Solos,
) -> Result<()> {
    let wanted = wanted_split(tx, actor, credit, name, solos).await?;
    let current: Vec<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT name,join_phrase,role FROM account_credit_splits WHERE user_id=? AND artist_id=? ORDER BY position",
    )
    .bind(actor.id())
    .bind(credit)
    .fetch_all(&mut **tx)
    .await?;
    let outcome = match &wanted {
        Some(parts)
            if parts.len() == current.len()
                && parts
                    .iter()
                    .zip(&current)
                    .all(|(part, (name, join, role))| {
                        &part.name == name && &part.join == join && role_name(part.role) == role
                    }) =>
        {
            return Ok(());
        }
        None if current.is_empty() => return Ok(()),
        None if chosen_by_hand(tx, actor, credit).await? => {
            tracing::debug!(
                outcome = "kept",
                "Credit split kept for the listener's member choices"
            );
            return Ok(());
        }
        Some(_) => "split",
        None => "removed",
    };
    sqlx::query("DELETE FROM account_credit_splits WHERE user_id=? AND artist_id=?")
        .bind(actor.id())
        .bind(credit)
        .execute(&mut **tx)
        .await?;
    for (position, part) in wanted.iter().flatten().enumerate() {
        sqlx::query(
            "INSERT INTO account_credit_splits(user_id,artist_id,position,name,join_phrase,role) VALUES(?,?,?,?,?,?)",
        )
        .bind(actor.id())
        .bind(credit)
        .bind(position as i64)
        .bind(&part.name)
        .bind(&part.join)
        .bind(role_name(part.role))
        .execute(&mut **tx)
        .await?;
    }
    crate::metadata::refresh_credit_members(tx, actor.id(), credit).await?;
    tracing::debug!(
        outcome,
        members = wanted.as_ref().map_or(0, Vec::len),
        "Credit split reconsidered"
    );
    Ok(())
}

async fn chosen_by_hand(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM recording_artist_member_identity_choices c
            JOIN account_recording_credits whole ON whole.user_id=c.user_id AND whole.recording_id=c.recording_id
            WHERE c.user_id=? AND whole.artist_id=? AND c.retired=0 AND NOT {AUTOMATIC_MEMBER_CHOICE})"
    ))
    .bind(actor.id())
    .bind(credit)
    .fetch_one(connection)
    .await?)
}

async fn unresolved(
    connection: &mut SqliteConnection,
    actor: &Actor,
    after: &str,
) -> Result<(Vec<String>, Option<String>)> {
    let mut rows: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT DISTINCT member.recording_id FROM account_recording_credit_members member
         JOIN recordings r ON r.id=member.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=member.user_id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=member.user_id
         WHERE member.user_id=? AND member.recording_id>? AND p.artist_operation_id IS NULL
            AND NOT EXISTS (SELECT 1 FROM recording_artist_member_identity_choices c
                WHERE c.user_id=member.user_id AND c.recording_id=member.recording_id
                    AND c.artist_id=member.artist_id AND NOT {MEMBER_REPLACEABLE})
         ORDER BY member.recording_id LIMIT ?"
    ))
    .bind(actor.id())
    .bind(after)
    .bind(MAX_RECORDINGS + 1)
    .fetch_all(connection)
    .await?;
    let next = if rows.len() as i64 > MAX_RECORDINGS {
        rows.truncate(MAX_RECORDINGS as usize);
        rows.last().cloned()
    } else {
        None
    };
    Ok((rows, next))
}

type MemberRow = (
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<bool>,
    Option<String>,
);

async fn members(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    credit: &Credit,
) -> Result<Vec<Member>> {
    let rows: Vec<MemberRow> = sqlx::query_as(&format!(
        "SELECT member.artist_id,artist.name,member.role,
            (SELECT CASE WHEN count(*)=1 THEN max(listed.provider_artist_id) END
                FROM recording_credited_artist_observations listed
                WHERE listed.observation_id=?3 AND listed.recording_id=member.recording_id
                    AND listed.name=artist.name COLLATE BINARY AND listed.role=member.role),
            c.revision,{MEMBER_REPLACEABLE},
            (SELECT g.provider_artist_id FROM account_artist_identity_groups g
                WHERE g.user_id=c.user_id AND g.id=c.group_id AND g.provider_kind='builtin_musicbrainz'
                    AND c.retired=0 AND c.association_id IS NOT NULL)
         FROM account_recording_credit_members member
         JOIN account_artists artist ON artist.user_id=member.user_id AND artist.id=member.artist_id
            AND artist.library_id=?4
         JOIN recordings r ON r.id=member.recording_id
         LEFT JOIN recording_artist_member_identity_choices c ON c.user_id=member.user_id
            AND c.recording_id=member.recording_id AND c.artist_id=member.artist_id
         WHERE member.user_id=?1 AND member.recording_id=?2 ORDER BY artist.name,member.artist_id"
    ))
    .bind(actor.id())
    .bind(recording)
    .bind(&credit.observation_id)
    .bind(&credit.library_id)
    .fetch_all(connection)
    .await?;
    let mut found = Vec::with_capacity(rows.len());
    for (artist, name, role, listed, revision, replaceable, decided) in rows {
        let Some(role) = role_of(&role) else {
            return Err(Error::Internal(
                "Stored artist credit member is invalid".into(),
            ));
        };
        let open = match (revision, replaceable) {
            (None, _) => Some(0),
            (Some(revision), Some(true)) => Some(revision),
            (Some(_), _) => None,
        };
        found.push(Member {
            artist,
            name,
            role,
            listed: listed
                .as_deref()
                .and_then(artist_id)
                .filter(|_| credit.source_id.is_some()),
            open,
            decided: if open.is_some() { None } else { decided },
        });
    }
    Ok(found)
}

async fn plan(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    policy_revision: i64,
    solos: &mut Solos,
) -> Result<Vec<Pending>> {
    let credit = match Credit::capture(tx, actor, recording).await {
        Ok(credit) if credit.credit_operation.is_none() => credit,
        Ok(_) | Err(Error::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let members = members(tx, actor, recording, &credit).await?;
    let mut evidence: Vec<Option<(String, MemberEvidence)>> = Vec::with_capacity(members.len());
    let mut conflicted = false;
    for member in &members {
        if member.open.is_none() {
            evidence.push(None);
            continue;
        }
        let solo = solos.identity(tx, actor, &member.name).await?;
        evidence.push(match (&member.listed, solo) {
            (Some(listed), Some(solo)) if *listed != solo => {
                conflicted = true;
                None
            }
            (Some(listed), _) => Some((listed.clone(), MemberEvidence::ListedArtistId)),
            (None, Some(solo)) => Some((solo, MemberEvidence::SoloCredit)),
            (None, None) => None,
        });
    }
    if credit.source_id.is_some() && !conflicted {
        let tags: Option<String> =
            sqlx::query_scalar("SELECT tags FROM metadata_observations WHERE id=?")
                .bind(&credit.observation_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        if let Some(tagged) = tagged_ids(tags.as_deref())
            && tagged.len() == members.len()
        {
            let known: Vec<Option<&String>> = members
                .iter()
                .zip(&evidence)
                .map(|(member, found)| found.as_ref().map(|(id, _)| id).or(member.decided.as_ref()))
                .collect();
            let unknown: Vec<usize> = (0..members.len())
                .filter(|index| known[*index].is_none())
                .collect();
            let left: Vec<&String> = tagged
                .iter()
                .filter(|id| !known.contains(&Some(*id)))
                .collect();
            let consistent = known.iter().flatten().all(|id| tagged.contains(*id));
            if let ([last], [id]) = (unknown.as_slice(), left.as_slice())
                && consistent
                && members[*last].open.is_some()
            {
                evidence[*last] = Some(((*id).clone(), MemberEvidence::FileTagArtistId));
            }
        }
    }
    let mut pending = Vec::new();
    for (member, found) in members.iter().zip(evidence) {
        let (Some(revision), Some((provider_artist_id, kind))) = (member.open, found) else {
            continue;
        };
        if let Some(item) = associate(
            tx,
            actor,
            recording,
            &credit,
            member,
            revision,
            (provider_artist_id, kind),
            policy_revision,
        )
        .await?
        {
            pending.push(item);
        }
    }
    Ok(pending)
}

#[allow(clippy::too_many_arguments)]
async fn associate(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    credit: &Credit,
    member: &Member,
    revision: i64,
    (provider_artist_id, evidence): (String, MemberEvidence),
    policy_revision: i64,
) -> Result<Option<Pending>> {
    let Some(revision) = revision.checked_add(1) else {
        return Ok(None);
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
    .bind(&provider_artist_id)
    .fetch_one(&mut *tx)
    .await?;
    if hidden {
        return Ok(None);
    }
    let target = CreditTarget::NamedMember {
        artist_id: member.artist.clone(),
        role: member.role,
    };
    let request = AssociateRecordingArtist {
        artist_id: member.artist.clone(),
        source_id: credit.source_id.clone(),
        observation_id: credit.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: MetadataProvider::BuiltinMusicBrainz,
        provider_artist_id: provider_artist_id.clone(),
        policy_revision,
    };
    let reused_order = match associations::existing_for_target(
        tx, actor, recording, &request, &target,
    )
    .await?
    {
        Some(existing) => {
            let order: Option<i64> = sqlx::query_scalar(
                "SELECT receipt_order FROM artist_association_member_matches WHERE association_id=?",
            )
            .bind(&existing)
            .fetch_optional(&mut *tx)
            .await?;
            match order {
                Some(order) if association_changes::is_linked(tx, &existing).await? => {
                    Some((existing, order))
                }
                _ => return Ok(None),
            }
        }
        None => None,
    };
    let (association, reused_order) = match reused_order {
        Some((association, order)) => (association, Some(order)),
        None => {
            let mut attempt = tx.begin().await?;
            match associations::retain_target(
                &mut attempt,
                actor,
                recording,
                &credit.library_id,
                &request,
                &target,
            )
            .await
            {
                Ok(association) => {
                    attempt.commit().await?;
                    (association.id, None)
                }
                Err(Error::Conflict(_) | Error::NotFound) => {
                    attempt.rollback().await?;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
    };
    Ok(Some(Pending {
        recording: recording.into(),
        artist: member.artist.clone(),
        role: member.role,
        revision,
        association,
        provider_artist_id,
        observation: credit.observation_id.clone(),
        reused_order,
        evidence,
    }))
}

async fn commit(
    tx: &mut SqliteConnection,
    actor: &Actor,
    pending: &[Pending],
    order: i64,
) -> Result<()> {
    let (provider, installation) = provider_columns(&MetadataProvider::BuiltinMusicBrainz);
    for item in pending {
        let group = choices::ensure_group(
            &mut *tx,
            actor,
            provider,
            installation,
            &item.provider_artist_id,
        )
        .await?;
        let selection = item.reused_order.unwrap_or(order);
        sqlx::query(
            "INSERT INTO recording_artist_member_identity_choices(user_id,recording_id,artist_id,role,revision,
                association_id,group_id,artist_operation_id,observation_id,selection_order)
             VALUES(?,?,?,?,?,?,?,NULL,?,?) ON CONFLICT(user_id,recording_id,artist_id) DO UPDATE SET
                role=excluded.role,revision=excluded.revision,association_id=excluded.association_id,
                group_id=excluded.group_id,artist_operation_id=NULL,
                observation_id=excluded.observation_id,retired=0,selected_at=unixepoch(),selection_order=excluded.selection_order",
        )
        .bind(actor.id())
        .bind(&item.recording)
        .bind(&item.artist)
        .bind(role_name(item.role))
        .bind(item.revision)
        .bind(&item.association)
        .bind(&group)
        .bind(&item.observation)
        .bind(selection)
        .execute(&mut *tx)
        .await?;
        if item.reused_order.is_none() {
            sqlx::query(
                "INSERT INTO artist_association_member_matches(association_id,evidence_kind,receipt_order) VALUES(?,?,?)",
            )
            .bind(&item.association)
            .bind(item.evidence.name())
            .bind(order)
            .execute(&mut *tx)
            .await?;
        }
    }
    tracing::debug!(chosen = pending.len(), "Credit member identities chosen");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{probes, tagged_ids};

    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn a_name_is_looked_up_as_written_spaced_once_and_normalized() {
        assert_eq!(probes("  Ada   Bo "), ["Ada   Bo", "Ada Bo", "ada bo"]);
        assert_eq!(probes("Ólafur"), ["Ólafur", "ólafur"]);
        assert!(probes("   ").is_empty());
    }

    #[test]
    fn identifier_tags_are_read_only_when_every_value_is_an_identifier() {
        let tags = |values: &[&str]| {
            let items: Vec<String> = values
                .iter()
                .map(|value| {
                    format!(r#"{{"role":"artist","key":"k","value":"{value}","location":{{"kind":"format"}}}}"#)
                })
                .collect();
            format!(r#"{{"musicbrainz_artist_tags":[{}]}}"#, items.join(","))
        };
        assert_eq!(
            tagged_ids(Some(&tags(&[&format!("{A};{B}")]))).map(|ids| ids.len()),
            Some(2)
        );
        assert_eq!(
            tagged_ids(Some(&tags(&[A, B, A]))).map(|ids| ids.len()),
            Some(2)
        );
        assert_eq!(tagged_ids(Some(&tags(&[A, "not-an-id"]))), None);
        assert_eq!(tagged_ids(None), None);
    }
}
