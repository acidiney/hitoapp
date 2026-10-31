//! The pure half of name matching: which exact-name artists a search proves it saw, which library
//! recordings to ask each of them about, and what their answers corroborate.
use hito_metadata_musicbrainz::{ArtistRecordings, NameSearch, RecordingProbe};

use super::{
    provider_evidence::SPECIAL_PURPOSE_ARTISTS,
    title::{EXACT_TOLERANCE_MS, Title, VERSION_TOLERANCE_MS, tolerance_ms},
};

/// More exact-name artists than this cannot be told apart within one request.
pub(super) const MAX_NAMESAKES: usize = 3;
/// Library recordings asked about one artist in one search, which is also the sample that decides.
pub(super) const PROBES_PER_SEARCH: usize = hito_metadata_musicbrainz::MAX_PROBES;
/// A shorter length is a placeholder, not a measurement.
const MIN_KNOWN_LENGTH_MS: u64 = 1000;
/// Distinct base titles a library must share with an artist to corroborate it.
pub(super) const MIN_CORROBORATING: usize = 2;
const SCORE_MAX: u32 = 100;
/// The longest title MusicBrainz is asked about; a longer one cannot be a request.
const MAX_TITLE_BYTES: usize = 512;

pub(super) struct Track {
    pub recording: String,
    pub title: String,
    pub length_ms: Option<u64>,
    pub observation: String,
    pub operation: Option<String>,
    /// The recording carries an ISRC, an artist page or a tagged artist identifier of its own.
    pub own_evidence: bool,
}

/// Case-folded, whitespace-collapsed text. Punctuation and diacritics are significant.
pub(super) fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(super) fn known_length(length: Option<i64>) -> Option<u64> {
    length
        .and_then(|length| u64::try_from(length).ok())
        .filter(|length| *length >= MIN_KNOWN_LENGTH_MS)
}

/// The ordinary artists whose name equals the credit, whatever their score, or `None` when the
/// search cannot prove it saw every one of them: too many namesakes, or a page that may end
/// before the last exact name. A page is conclusive when it holds the whole result set, or ends on
/// a result that is not an exact name and scores below the maximum.
pub(super) fn namesakes(search: &NameSearch, credit: &str) -> Option<Vec<String>> {
    let wanted = normalize(credit);
    let exact = |name: &str| normalize(name) == wanted;
    let complete = search.items.len() as u32 >= search.total;
    let ends_clear = search
        .items
        .last()
        .is_some_and(|last| last.score < SCORE_MAX && !exact(&last.name));
    if !(complete || ends_clear) {
        return None;
    }
    let found: Vec<String> = search
        .items
        .iter()
        .filter(|item| exact(&item.name))
        .map(|item| item.id.clone())
        .filter(|id| !SPECIAL_PURPOSE_ARTISTS.contains(&id.as_str()))
        .collect();
    (found.len() <= MAX_NAMESAKES).then_some(found)
}

/// One title and length of the credit's undecided recordings, with the tracks that carry them.
pub(super) struct Candidate {
    pub title: Title,
    pub length_ms: u64,
    pub tracks: Vec<usize>,
}

/// The distinct (title, length) pairs of tracks with a known length, in track order.
pub(super) fn candidates<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    for (index, track) in tracks.into_iter().enumerate() {
        let Some(length) = track
            .length_ms
            .and_then(|l| known_length(i64::try_from(l).ok()))
        else {
            continue;
        };
        let title = Title::parse(&track.title);
        if title.text.is_empty() || title.text.len() > MAX_TITLE_BYTES {
            continue;
        }
        match found
            .iter_mut()
            .find(|known| known.title == title && known.length_ms == length)
        {
            Some(known) => known.tracks.push(index),
            None => found.push(Candidate {
                title,
                length_ms: length,
                tracks: vec![index],
            }),
        }
    }
    found
}

/// The deciding sample: one candidate for each of the first distinct base titles, at most
/// `PROBES_PER_SEARCH`. A title without a version qualifier is preferred to a version of the same
/// song, because it is asked about with the narrower length range.
pub(super) fn sample(candidates: &[Candidate]) -> Vec<usize> {
    let mut picked: Vec<usize> = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let same_song = picked
            .iter()
            .position(|known| candidates[*known].title.key == candidate.title.key);
        match same_song {
            Some(slot) => {
                if candidates[picked[slot]].title.is_qualified() && !candidate.title.is_qualified()
                {
                    picked[slot] = index;
                }
            }
            None if picked.len() < PROBES_PER_SEARCH => picked.push(index),
            None => {}
        }
    }
    picked
}

pub(super) fn probes(candidates: &[Candidate], picked: &[usize]) -> Vec<RecordingProbe> {
    picked
        .iter()
        .map(|index| {
            let candidate = &candidates[*index];
            let tolerance = if candidate.title.is_qualified() {
                VERSION_TOLERANCE_MS
            } else {
                EXACT_TOLERANCE_MS
            };
            RecordingProbe {
                title: candidate.title.text.clone(),
                min_length_ms: candidate.length_ms.saturating_sub(tolerance),
                max_length_ms: candidate.length_ms + tolerance,
            }
        })
        .collect()
}

/// The picked candidates that a listed recording equals by title and by length within the
/// tolerance the two titles allow.
pub(super) fn found(
    candidates: &[Candidate],
    picked: &[usize],
    listed: &ArtistRecordings,
) -> Vec<usize> {
    let listed: Vec<(Title, u64)> = listed
        .items
        .iter()
        .filter_map(|item| {
            let length = known_length(i64::try_from(item.length_ms?).ok())?;
            Some((Title::parse(&item.title), length))
        })
        .collect();
    picked
        .iter()
        .copied()
        .filter(|index| {
            let candidate = &candidates[*index];
            listed.iter().any(|(title, length)| {
                tolerance_ms(&candidate.title, title)
                    .is_some_and(|tolerance| length.abs_diff(candidate.length_ms) <= tolerance)
            })
        })
        .collect()
}

/// How many distinct base titles the given candidates carry.
pub(super) fn distinct_titles(candidates: &[Candidate], hits: &[usize]) -> usize {
    let mut titles: Vec<&str> = Vec::new();
    for index in hits {
        let title = candidates[*index].title.key.as_str();
        if !titles.contains(&title) {
            titles.push(title);
        }
    }
    titles.len()
}

/// What one exact-name artist's answer says about the sample.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Assessment {
    Corroborated(Vec<usize>),
    /// Fewer than two titles listed, with the sample candidates that were.
    Absent(Vec<usize>),
    /// The answer holds fewer matches than MusicBrainz states and did not corroborate.
    NotRead,
}

pub(super) fn assess(
    candidates: &[Candidate],
    picked: &[usize],
    listed: &ArtistRecordings,
) -> Assessment {
    let hits = found(candidates, picked, listed);
    if distinct_titles(candidates, &hits) >= MIN_CORROBORATING {
        Assessment::Corroborated(hits)
    } else if listed.complete() {
        Assessment::Absent(hits)
    } else {
        Assessment::NotRead
    }
}

/// The artist, with the sample candidates it lists, when exactly one namesake corroborates and no
/// other could still do so with matches that were not read. An artist already matched for the
/// credit by an earlier request continues without fresh corroboration when no other namesake
/// listed any sampled title.
pub(super) fn decide(
    assessments: &[(String, Assessment)],
    established: Option<&str>,
) -> Option<(String, Vec<usize>)> {
    let mut accepted = None;
    for (id, assessment) in assessments {
        match assessment {
            Assessment::Corroborated(hits) if accepted.is_none() => {
                accepted = Some((id.clone(), hits.clone()));
            }
            Assessment::Corroborated(_) | Assessment::NotRead => return None,
            Assessment::Absent(_) => {}
        }
    }
    match (accepted, established) {
        (Some((id, hits)), Some(established)) => (id == established).then_some((id, hits)),
        (Some(accepted), None) => Some(accepted),
        (None, Some(established)) => {
            let continuing = assessments
                .iter()
                .find_map(|(id, assessment)| match assessment {
                    Assessment::Absent(hits) if id == established => Some(hits.clone()),
                    _ => None,
                })?;
            silent_beside(assessments, established).then(|| (established.to_owned(), continuing))
        }
        (None, None) => None,
    }
}

/// Whether every namesake other than the artist listed none of the sampled titles.
pub(super) fn silent_beside(assessments: &[(String, Assessment)], artist: &str) -> bool {
    assessments.iter().all(|(id, assessment)| {
        id == artist || matches!(assessment, Assessment::Absent(hits) if hits.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hito_metadata_musicbrainz::{ArtistRecording, NameCandidate};

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";
    const THIRD: &str = "33333333-3333-4333-8333-333333333333";
    const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

    fn page(names: &[(&str, &str, u32)], total: u32) -> NameSearch {
        NameSearch {
            total,
            items: names
                .iter()
                .map(|(id, name, score)| NameCandidate {
                    id: (*id).into(),
                    name: (*name).into(),
                    score: *score,
                })
                .collect(),
        }
    }

    #[test]
    fn names_compare_by_case_and_spacing_but_not_by_punctuation_or_accents() {
        assert_eq!(normalize("  Nova   QUARTET "), "nova quartet");
        assert_ne!(normalize("Nova Quartet"), normalize("Nova-Quartet"));
        assert_ne!(normalize("Beyoncé"), normalize("Beyonce"));
    }

    #[test]
    fn exact_names_are_namesakes_whatever_their_score() {
        let live = [
            (ID, "Ed Sheeran", 100),
            (OTHER, "Ed Sheeran", 73),
            (THIRD, "ed  sheeran", 72),
            (
                "44444444-4444-4444-8444-444444444444",
                "Ed Sheeran Tribute",
                49,
            ),
        ];
        assert_eq!(
            namesakes(&page(&live, 4), "Ed Sheeran"),
            Some(vec![ID.into(), OTHER.into(), THIRD.into()]),
            "The live search found three exact names scoring 100, 73 and 72"
        );
        assert_eq!(
            namesakes(&page(&live, 4_000), "Ed Sheeran"),
            Some(vec![ID.into(), OTHER.into(), THIRD.into()]),
            "A page that ends on a lower-scored result that is no exact name is conclusive"
        );
    }

    #[test]
    fn a_page_that_may_end_before_the_last_exact_name_names_none() {
        let full = [(ID, "Nova Quartet", 100), (OTHER, "NOVA quartet", 100)];
        assert_eq!(
            namesakes(&page(&full, 2), "Nova Quartet"),
            Some(vec![ID.into(), OTHER.into()])
        );
        assert_eq!(
            namesakes(&page(&full, 150), "Nova Quartet"),
            None,
            "A page ending at the maximum score may hide an exact name"
        );
        let ends_exact = [(ID, "Nova Quartet", 100), (OTHER, "Nova Quartet", 80)];
        assert_eq!(
            namesakes(&page(&ends_exact, 400), "Nova Quartet"),
            None,
            "A page that ends on an exact name may hide another one"
        );
        let many = [
            (ID, "A", 100),
            (OTHER, "A", 100),
            (THIRD, "A", 100),
            ("44444444-4444-4444-8444-444444444444", "A", 100),
        ];
        assert_eq!(namesakes(&page(&many, 4), "A"), None);
        assert_eq!(
            namesakes(
                &page(&[(VARIOUS_ARTISTS, "Various Artists", 100)], 1),
                "Various Artists"
            ),
            Some(vec![])
        );
    }

    fn track(title: &str, length: Option<u64>) -> Track {
        Track {
            recording: title.into(),
            title: title.into(),
            length_ms: length,
            observation: "o".into(),
            operation: None,
            own_evidence: false,
        }
    }

    fn listed(items: &[(&str, u64)], total: u32) -> ArtistRecordings {
        ArtistRecordings {
            total,
            items: items
                .iter()
                .enumerate()
                .map(|(index, (title, length))| ArtistRecording {
                    id: format!("0f700000-0000-4000-8000-{index:012x}"),
                    title: (*title).into(),
                    length_ms: Some(*length),
                })
                .collect(),
        }
    }

    #[test]
    fn candidates_skip_unknown_lengths_and_the_sample_takes_one_per_title() {
        let tracks = [
            track("Intro", Some(30_000)),
            track("INTRO", Some(30_000)),
            track("Intro", Some(90_000)),
            track("Unknown", None),
            track("Short", Some(999)),
            track("Outro", Some(60_000)),
        ];
        let found = candidates(&tracks);
        assert_eq!(found.len(), 3);
        assert_eq!(
            found[0].tracks,
            vec![0, 1],
            "Equal titles and lengths share a candidate"
        );
        assert_eq!(
            sample(&found),
            vec![0, 2],
            "One candidate per distinct title"
        );
        let many: Vec<Track> = (0..10)
            .map(|index| track(&format!("Song {index}"), Some(100_000)))
            .collect();
        let found = candidates(&many);
        assert_eq!(sample(&found), (0..6).collect::<Vec<_>>());
        let probes = probes(&found, &sample(&found)[..1]);
        assert_eq!(
            (probes[0].min_length_ms, probes[0].max_length_ms),
            (98_000, 102_000)
        );
    }

    #[test]
    fn the_sample_prefers_the_plain_title_of_a_song_to_its_versions() {
        let tracks = [
            track("Song (Live)", Some(100_000)),
            track("Other", Some(90_000)),
            track("Song - Remix", Some(100_000)),
            track("Song", Some(101_000)),
        ];
        let found = candidates(&tracks);
        assert_eq!(sample(&found), vec![3, 1]);
        let probes = probes(&found, &sample(&found));
        assert_eq!(
            (
                probes[0].title.as_str(),
                probes[0].min_length_ms,
                probes[0].max_length_ms
            ),
            ("Song", 99_000, 103_000),
            "A plain title is asked about with the narrow range"
        );
    }

    #[test]
    fn two_recordings_sharing_one_title_do_not_corroborate() {
        let tracks = [track("Intro", Some(30_000)), track("Intro", Some(90_000))];
        let found = candidates(&tracks);
        let picked = [0, 1];
        let hits = super::found(
            &found,
            &picked,
            &listed(&[("Intro", 30_000), ("Intro", 90_000)], 2),
        );
        assert_eq!(distinct_titles(&found, &hits), 1);
        assert_eq!(
            assess(
                &found,
                &picked,
                &listed(&[("Intro", 30_000), ("Intro", 90_000)], 2)
            ),
            Assessment::Absent(vec![0, 1])
        );
        let tracks = [track("Intro", Some(30_000)), track("Outro", Some(90_000))];
        let found = candidates(&tracks);
        assert_eq!(
            assess(
                &found,
                &[0, 1],
                &listed(&[("Intro", 30_000), ("outro", 91_999)], 2)
            ),
            Assessment::Corroborated(vec![0, 1])
        );
        assert_eq!(
            assess(
                &found,
                &[0, 1],
                &listed(&[("Intro", 30_000), ("outro", 92_001)], 2)
            ),
            Assessment::Absent(vec![0]),
            "Two seconds is the widest tolerated difference"
        );
    }

    #[test]
    fn an_answer_cut_short_that_did_not_corroborate_was_not_fully_read() {
        let tracks = [track("Intro", Some(30_000)), track("Outro", Some(90_000))];
        let found = candidates(&tracks);
        assert_eq!(
            assess(&found, &[0, 1], &listed(&[("Intro", 30_000)], 60)),
            Assessment::NotRead
        );
        assert_eq!(
            assess(
                &found,
                &[0, 1],
                &listed(&[("Intro", 30_000), ("Outro", 90_000)], 60)
            ),
            Assessment::Corroborated(vec![0, 1]),
            "A corroborating answer stands however much was left unread"
        );
    }

    #[test]
    fn exactly_one_corroborated_namesake_with_no_unread_one_is_accepted() {
        let yes = || Assessment::Corroborated(vec![0, 1]);
        let absent = || Assessment::Absent(vec![]);
        assert_eq!(
            decide(&[(ID.into(), yes()), (OTHER.into(), absent())], None),
            Some((ID.into(), vec![0, 1]))
        );
        assert_eq!(
            decide(&[(ID.into(), yes()), (OTHER.into(), yes())], None),
            None
        );
        assert_eq!(
            decide(
                &[(ID.into(), yes()), (OTHER.into(), Assessment::NotRead)],
                None
            ),
            None
        );
        assert_eq!(decide(&[(ID.into(), absent())], None), None);
    }

    #[test]
    fn an_artist_already_matched_for_the_credit_continues_only_beside_silent_namesakes() {
        let one = || Assessment::Absent(vec![3]);
        let none = || Assessment::Absent(vec![]);
        assert_eq!(
            decide(&[(ID.into(), one()), (OTHER.into(), none())], Some(ID)),
            Some((ID.into(), vec![3]))
        );
        assert_eq!(
            decide(&[(ID.into(), one()), (OTHER.into(), one())], Some(ID)),
            None,
            "A namesake that lists a sampled title is not silent"
        );
        assert_eq!(
            decide(&[(ID.into(), none())], Some(OTHER)),
            None,
            "The established artist must be among the namesakes"
        );
        assert_eq!(
            decide(
                &[
                    (ID.into(), Assessment::Corroborated(vec![0, 1])),
                    (OTHER.into(), none())
                ],
                Some(OTHER)
            ),
            None,
            "Corroboration of a different artist than the established one decides nothing"
        );
        assert!(silent_beside(
            &[(ID.into(), one()), (OTHER.into(), none())],
            ID
        ));
        assert!(!silent_beside(
            &[(ID.into(), none()), (OTHER.into(), one())],
            ID
        ));
    }
}
