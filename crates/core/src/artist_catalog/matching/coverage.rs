//! Coverage: when one exact-name artist is corroborated by most of the tracks of a credit that
//! were asked about, the credit's remaining undecided tracks follow that artist. This module holds
//! what an earlier request already settled for the credit, the arithmetic, and the check that
//! strong evidence of the credit does not name another artist.
use super::{apply::EvidenceKind, credit::strong_votes, name_probe::Track, title::Title};
use crate::artist_catalog::identity::groups::current_choices;
use crate::{Actor, Result};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

/// Distinct base titles the artist must have been corroborated by.
const MIN_TITLES: usize = 5;
/// The artist must list at least this many of every ten distinct titles that were asked about.
const MIN_TENTHS: usize = 7;
/// Name matches made under an earlier rule do not count toward an artist's coverage.
const MIN_COUNTING_RULE: i64 = 3;

/// Checked tracks of one credit that may follow in a single settlement.
const MAX_CHECKED: i64 = 1000;

#[derive(sqlx::FromRow)]
struct CheckedRow {
    id: String,
    title: String,
    duration_ms: Option<i64>,
    metadata_observation_id: String,
    artist_operation_id: Option<String>,
    own_evidence: bool,
    tagged_recording: bool,
}

/// A track of the credit that an earlier request matched to the artist by name.
pub(super) struct Matched {
    pub title: String,
    pub length_ms: Option<u64>,
}

/// What earlier requests settled for a credit: the artists its tracks were matched to by name, and
/// the undecided tracks that were asked about and found nowhere.
#[derive(Default)]
pub(super) struct Standing {
    artists: Vec<String>,
    pub matched: Vec<Matched>,
    pub checked: Vec<Track>,
}

impl Standing {
    /// The one artist every name match of the credit names, or `None`.
    pub(super) fn established(&self) -> Option<&str> {
        match self.artists.as_slice() {
            [only] => Some(only.as_str()),
            _ => None,
        }
    }

    /// The matched tracks as library tracks, to ask other namesakes about them.
    pub(super) fn tracks(&self) -> Vec<Track> {
        self.matched
            .iter()
            .map(|matched| Track {
                recording: String::new(),
                title: matched.title.clone(),
                length_ms: matched.length_ms,
                observation: String::new(),
                operation: None,
                own_evidence: false,
                tagged_recording: false,
            })
            .collect()
    }
}

pub(super) async fn standing(
    connection: &mut SqliteConnection,
    actor: &Actor,
    credit: &str,
    rule: i64,
) -> Result<Standing> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT (SELECT g.provider_artist_id FROM account_artist_identity_groups g
            WHERE g.user_id=choice.user_id AND g.id=choice.group_id),r.title,r.duration_ms ",
    );
    query.push(current_choices());
    query
        .push_bind(actor.id().to_owned())
        .push(" AND choice.artist_id=")
        .push_bind(credit.to_owned())
        .push(
            " AND EXISTS (SELECT 1 FROM artist_association_name_matches nm
                WHERE nm.association_id=choice.association_id AND nm.receipt_order=choice.selection_order
                    AND nm.rule>=",
        )
        .push_bind(MIN_COUNTING_RULE)
        .push(")");
    let rows: Vec<(String, String, Option<i64>)> =
        query.build_query_as().fetch_all(&mut *connection).await?;
    let mut standing = Standing::default();
    for (artist, title, length) in rows {
        if !standing.artists.contains(&artist) {
            standing.artists.push(artist);
        }
        standing.matched.push(Matched {
            title,
            length_ms: length.and_then(|length| u64::try_from(length).ok()),
        });
    }
    let checked: Vec<CheckedRow> = sqlx::query_as(
        "SELECT r.id,r.title,r.duration_ms,r.metadata_observation_id,p.artist_operation_id,
            (EXISTS (SELECT 1 FROM recording_isrc_observations i WHERE i.observation_id=r.metadata_observation_id)
                OR EXISTS (SELECT 1 FROM recording_artist_url_observations u WHERE u.observation_id=r.metadata_observation_id)
                OR EXISTS (SELECT 1 FROM recording_musicbrainz_observations t WHERE t.observation_id=r.metadata_observation_id AND t.recording_id=r.id AND t.entity='recording')
                OR COALESCE(json_array_length((SELECT json_extract(o.tags,'$.musicbrainz_artist_tags')
                    FROM metadata_observations o WHERE o.id=r.metadata_observation_id)),0)>0) AS own_evidence,
            (EXISTS (SELECT 1 FROM recording_musicbrainz_observations t WHERE t.observation_id=r.metadata_observation_id AND t.recording_id=r.id AND t.entity='recording')) AS tagged_recording
         FROM account_recording_credits rc
         JOIN recordings r ON r.id=rc.recording_id
         JOIN library_members m ON m.library_id=r.library_id AND m.user_id=rc.user_id
         LEFT JOIN personal_recording_metadata p ON p.recording_id=r.id AND p.user_id=rc.user_id
         JOIN artist_name_checks k ON k.user_id=rc.user_id AND k.recording_id=r.id
            AND k.observation_id=r.metadata_observation_id AND k.artist_operation_id IS p.artist_operation_id
            AND k.rule=?
         WHERE rc.user_id=? AND rc.artist_id=? AND NOT EXISTS (SELECT 1 FROM recording_artist_identity_choices c
            WHERE c.user_id=rc.user_id AND c.recording_id=r.id)
         ORDER BY r.id LIMIT ?",
    )
    .bind(rule)
    .bind(actor.id())
    .bind(credit)
    .bind(MAX_CHECKED)
    .fetch_all(connection)
    .await?;
    standing.checked = checked
        .into_iter()
        .map(|row| Track {
            recording: row.id,
            title: row.title,
            length_ms: row
                .duration_ms
                .and_then(|length| u64::try_from(length).ok()),
            observation: row.metadata_observation_id,
            operation: row.artist_operation_id,
            own_evidence: row.own_evidence,
            tagged_recording: row.tagged_recording,
        })
        .collect();
    Ok(standing)
}

/// How much of the credit one artist covers.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Tally {
    /// Distinct base titles the artist has been matched on.
    pub titles: usize,
    /// Distinct base titles of the tracks that were asked about, matched or not.
    pub probed_titles: usize,
    /// Tracks of the credit matched to the artist by name, now and earlier.
    pub matched: usize,
    /// Tracks of the credit that were asked about: the matched ones and those found nowhere.
    pub probed: usize,
}

impl Tally {
    /// Both thresholds count distinct songs, so editions of one song do not inflate the share.
    pub(super) fn qualifies(&self) -> bool {
        self.titles >= MIN_TITLES && self.titles * 10 >= self.probed_titles * MIN_TENTHS
    }

    /// The track counts shown to the listener.
    pub(super) fn counts(&self) -> (u32, u32) {
        (
            u32::try_from(self.matched).unwrap_or(u32::MAX),
            u32::try_from(self.probed).unwrap_or(u32::MAX),
        )
    }
}

fn add_key(keys: &mut Vec<String>, title: &str) {
    let key = Title::parse(title).key;
    if !keys.contains(&key) {
        keys.push(key);
    }
}

/// The tally for a credit all of whose tracks of this request were asked about, `matched_now` of
/// them (positions in `tracks`) matched to the artist.
pub(super) fn tally(standing: &Standing, tracks: &[Track], matched_now: &[usize]) -> Tally {
    let mut matched_titles: Vec<String> = Vec::new();
    for matched in &standing.matched {
        add_key(&mut matched_titles, &matched.title);
    }
    for index in matched_now {
        add_key(&mut matched_titles, &tracks[*index].title);
    }
    let mut probed_titles = matched_titles.clone();
    for track in &standing.checked {
        add_key(&mut probed_titles, &track.title);
    }
    for (index, track) in tracks.iter().enumerate() {
        if !matched_now.contains(&index) {
            add_key(&mut probed_titles, &track.title);
        }
    }
    let matched = standing.matched.len() + matched_now.len();
    Tally {
        titles: matched_titles.len(),
        probed_titles: probed_titles.len(),
        matched,
        probed: matched + standing.checked.len() + tracks.len() - matched_now.len(),
    }
}

async fn credits_of(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT artist_id FROM account_recording_credits WHERE user_id=? AND recording_id=?",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_all(tx)
    .await?)
}

/// Whether strong evidence (file tag identifier, ISRC or artist page) of the credits of a
/// recording names an artist other than the given one.
async fn strong_conflict(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    artist: &str,
) -> Result<bool> {
    for credit in credits_of(tx, actor, recording).await? {
        let votes = strong_votes(tx, actor, &credit, "[]").await?;
        if votes.iter().any(|(identity, _)| identity != artist) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether any current choice in the credits of a recording, made by the account or by any pass,
/// names an artist other than the given one.
async fn other_choice(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    artist: &str,
) -> Result<bool> {
    for credit in credits_of(tx, actor, recording).await? {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT EXISTS (SELECT 1 ");
        query.push(current_choices());
        query
            .push_bind(actor.id().to_owned())
            .push(" AND choice.artist_id=")
            .push_bind(credit)
            .push(
                " AND (SELECT g.provider_artist_id FROM account_artist_identity_groups g
                    WHERE g.user_id=choice.user_id AND g.id=choice.group_id)<>",
            )
            .push_bind(artist.to_owned())
            .push(")");
        if query.build_query_scalar().fetch_one(&mut *tx).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether evidence that arrived or was decided since the walk looked refuses a name-based
/// match: strong evidence for another artist blocks any, and a choice for another artist in the
/// credit also blocks the credit's remaining tracks.
pub(super) async fn vetoed(
    tx: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    artist: &str,
    kind: EvidenceKind,
) -> Result<bool> {
    match kind {
        EvidenceKind::NameDiscography => strong_conflict(tx, actor, recording, artist).await,
        EvidenceKind::Coverage(..) => Ok(strong_conflict(tx, actor, recording, artist).await?
            || other_choice(tx, actor, recording, artist).await?),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str) -> Track {
        Track {
            recording: title.into(),
            title: title.into(),
            length_ms: Some(100_000),
            observation: "o".into(),
            operation: None,
            own_evidence: false,
            tagged_recording: false,
        }
    }

    fn now(count: usize, unmatched: usize) -> (Vec<Track>, Vec<usize>) {
        let tracks = (0..count + unmatched)
            .map(|index| track(&format!("Song {index}")))
            .collect();
        (tracks, (0..count).collect())
    }

    #[test]
    fn five_titles_and_seven_tenths_of_the_probed_tracks_qualify() {
        for (matched, unmatched, qualifies) in [
            (85, 21, true),
            (75, 31, true),
            (74, 32, false),
            (5, 2, true),
            (5, 3, false),
            (4, 0, false),
            (3, 103, false),
        ] {
            let (tracks, positions) = now(matched, unmatched);
            let tally = tally(&Standing::default(), &tracks, &positions);
            assert_eq!(
                tally.qualifies(),
                qualifies,
                "{matched} of {}",
                matched + unmatched
            );
        }
    }

    #[test]
    fn the_share_counts_distinct_titles_not_duplicates_of_one_song() {
        let mut tracks: Vec<Track> = Vec::new();
        for song in 0..5 {
            for _edition in 0..6 {
                tracks.push(track(&format!("Song {song}")));
            }
        }
        for namesake in 0..12 {
            tracks.push(track(&format!("Other {namesake}")));
        }
        let matched: Vec<usize> = (0..30).collect();
        let tally = tally(&Standing::default(), &tracks, &matched);
        assert!(
            !tally.qualifies(),
            "30 of 42 tracks is 71 percent but 5 of 17 songs is not"
        );
        assert!(
            super::tally(&Standing::default(), &tracks[..30], &matched).qualifies(),
            "Five songs that are all of the probed songs qualify"
        );
    }

    #[test]
    fn earlier_matches_and_checks_count_and_versions_of_one_song_are_one_title() {
        let standing = Standing {
            artists: vec!["a".into()],
            matched: vec![
                Matched {
                    title: "Perfect".into(),
                    length_ms: Some(1),
                },
                Matched {
                    title: "Perfect - Live".into(),
                    length_ms: Some(1),
                },
                Matched {
                    title: "Photograph".into(),
                    length_ms: Some(1),
                },
            ],
            checked: vec![track("Lost earlier")],
        };
        let tracks = vec![
            track("Shape of You (Acoustic)"),
            track("Castle on the Hill"),
            track("Lost"),
        ];
        let tally = tally(&standing, &tracks, &[0, 1]);
        assert_eq!(
            tally,
            Tally {
                titles: 4,
                probed_titles: 6,
                matched: 5,
                probed: 5 + 1 + 1
            },
            "Perfect and Perfect - Live are one title; Lost was asked about and not found"
        );
        assert_eq!(standing.established(), Some("a"));
        assert_eq!(Standing::default().established(), None);
    }
}
