//! Title comparison for name matching. Titles are folded and stripped of version qualifiers and
//! featured artists for comparison only; the stored and displayed titles never change. A featured
//! artist is recognized only in brackets or after a dash ("Song (feat. X)", "Song - feat. X"), never
//! in the middle of a title, and "with" is never read as one: the credit of the recording is not
//! known here, so "Song (With You)" stays "song with you".
use crate::artist_catalog::identity::groups::fold;

/// Length tolerance between a library title and a listed title of the same wording or base.
pub(super) const EXACT_TOLERANCE_MS: u64 = 2000;
/// Length tolerance when both titles carry the same kind of version qualifier.
pub(super) const VERSION_TOLERANCE_MS: u64 = 10_000;

const LIVE: u16 = 1;
const ACOUSTIC: u16 = 1 << 1;
const REMASTER: u16 = 1 << 2;
const REMIX: u16 = 1 << 3;
const EDIT: u16 = 1 << 4;
const VERSION: u16 = 1 << 5;
const CHANNELS: u16 = 1 << 6;
const DEMO: u16 = 1 << 7;
const INSTRUMENTAL: u16 = 1 << 8;
const EXPLICIT: u16 = 1 << 9;
const DELUXE: u16 = 1 << 10;

/// Words that, after "live", still describe a live version and not the rest of a title.
const LIVE_CONTINUATIONS: [&str; 8] = [
    "at",
    "in",
    "from",
    "on",
    "version",
    "session",
    "recording",
    "performance",
];
/// Words that may stand in a qualifier beside a kind word without adding a kind.
const FILLER: [&str; 17] = [
    "radio",
    "single",
    "album",
    "original",
    "digital",
    "track",
    "the",
    "extended",
    "short",
    "main",
    "uk",
    "us",
    "anniversary",
    "lp",
    "mix",
    "edition",
    "version",
];
const FEATURING: [&str; 3] = ["feat", "ft", "featuring"];

#[derive(Clone, Debug)]
pub(super) struct Title {
    /// The title with qualifiers and featured artists removed, as written.
    pub text: String,
    /// The folded base used to compare titles.
    pub key: String,
    kinds: u16,
}

/// Titles are equal when their base and qualifier kinds are; the wording is only for display.
impl PartialEq for Title {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.kinds == other.kinds
    }
}

impl Eq for Title {}

fn tokens(text: &str) -> Vec<String> {
    fold(text)
        .replace(['\'', '\u{2019}'], "")
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_number(word: &str) -> bool {
    word.starts_with(|character: char| character.is_ascii_digit())
}

fn kind_of(word: &str) -> Option<u16> {
    Some(match word {
        "acoustic" => ACOUSTIC,
        "remaster" | "remastered" => REMASTER,
        "edit" => EDIT,
        "demo" => DEMO,
        "instrumental" => INSTRUMENTAL,
        "mono" | "stereo" => CHANNELS,
        "explicit" | "clean" => EXPLICIT,
        "deluxe" | "bonus" | "expanded" => DELUXE,
        "live" => LIVE,
        _ => return None,
    })
}

/// What a bracketed or dash-separated part of a title says: the version kinds it names (none for
/// a featured artist), or `None` when it is part of the title itself.
fn qualifier(part: &str) -> Option<u16> {
    let words = tokens(part);
    let first = words.first()?;
    if FEATURING.contains(&first.as_str()) {
        return Some(0);
    }
    if first == "live"
        && words
            .get(1)
            .is_none_or(|next| LIVE_CONTINUATIONS.contains(&next.as_str()))
    {
        return Some(LIVE);
    }
    if words
        .iter()
        .any(|word| matches!(word.as_str(), "remix" | "remixed" | "rmx"))
    {
        return Some(REMIX);
    }
    let mut kinds = 0;
    for word in &words {
        match kind_of(word) {
            Some(kind) => kinds |= kind,
            None if FILLER.contains(&word.as_str()) || is_number(word) => {}
            None => return None,
        }
    }
    if kinds == 0 && words.iter().any(|word| word == "version") {
        kinds = VERSION;
    }
    (kinds != 0).then_some(kinds)
}

/// The start of the trailing bracketed part of the text, if it ends with one.
fn trailing_bracket(text: &str) -> Option<usize> {
    let closing = text.chars().last()?;
    let opening = match closing {
        ')' => '(',
        ']' => '[',
        _ => return None,
    };
    let mut depth = 0_u32;
    for (index, character) in text.char_indices().rev() {
        if character == closing {
            depth += 1;
        } else if character == opening {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// The start of the trailing part after the last dash separator.
fn trailing_dash(text: &str) -> Option<(usize, usize)> {
    ["\u{2013}", "\u{2014}", "-"]
        .iter()
        .filter_map(|dash| {
            text.rmatch_indices(&format!(" {dash} "))
                .next()
                .map(|(index, found)| (index, index + found.len()))
        })
        .max()
}

impl Title {
    pub(super) fn parse(raw: &str) -> Self {
        let mut base = raw.trim();
        let mut kinds = 0;
        loop {
            let cut = if let Some(start) = trailing_bracket(base) {
                qualifier(&base[start + 1..base.len() - 1]).map(|found| (start, found))
            } else if let Some((start, after)) = trailing_dash(base) {
                qualifier(&base[after..]).map(|found| (start, found))
            } else {
                None
            };
            match cut {
                Some((start, found)) if !base[..start].trim().is_empty() => {
                    kinds |= found;
                    base = base[..start].trim_end();
                }
                _ => break,
            }
        }
        Self {
            text: base.to_owned(),
            key: tokens(base).join(" "),
            kinds,
        }
    }

    pub(super) fn is_qualified(&self) -> bool {
        self.kinds != 0
    }
}

/// The length tolerance within which a listed title is the same recording as the library title,
/// or `None` when the titles do not match. A remix matches only remixes of the same base title;
/// any other qualified title matches the same qualifier kinds or the unqualified base title.
pub(super) fn tolerance_ms(candidate: &Title, listed: &Title) -> Option<u64> {
    if candidate.key != listed.key {
        return None;
    }
    if candidate.kinds & REMIX != 0 {
        return (listed.kinds & REMIX != 0).then_some(VERSION_TOLERANCE_MS);
    }
    if candidate.kinds == listed.kinds {
        return Some(if candidate.is_qualified() {
            VERSION_TOLERANCE_MS
        } else {
            EXACT_TOLERANCE_MS
        });
    }
    (listed.kinds == 0).then_some(EXACT_TOLERANCE_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(candidate: &str, listed: &str, difference_ms: u64) -> bool {
        tolerance_ms(&Title::parse(candidate), &Title::parse(listed))
            .is_some_and(|tolerance| difference_ms <= tolerance)
    }

    #[test]
    fn version_qualifiers_and_featured_artists_leave_the_base_title() {
        for (raw, base, qualified) in [
            ("Shape of You (Acoustic)", "shape of you", true),
            ("Perfect - Live", "perfect", true),
            ("Perfect \u{2013} Live", "perfect", true),
            (
                "Castle on the Hill [Remastered]",
                "castle on the hill",
                true,
            ),
            ("Hello - Remastered 2011", "hello", true),
            ("Hello (2011 Remaster)", "hello", true),
            (
                "Thinking Out Loud - Alex Ghenea Remix",
                "thinking out loud",
                true,
            ),
            ("Thinking Out Loud (Remix)", "thinking out loud", true),
            ("Song - Radio Edit", "song", true),
            ("Song (Edit)", "song", true),
            ("Song (Single Version)", "song", true),
            ("Song [Album Version]", "song", true),
            ("Song (Mono)", "song", true),
            ("Song (Stereo Mix)", "song", true),
            ("Song (Demo)", "song", true),
            ("Song (Instrumental)", "song", true),
            ("Song (Explicit)", "song", true),
            ("Song (Clean Version)", "song", true),
            ("Song (Deluxe Edition)", "song", true),
            ("Song (Bonus Track)", "song", true),
            ("Song [Live at Wembley Stadium]", "song", true),
            ("Song (Live) [Remastered 2009]", "song", true),
            ("Perfect (feat. Beyonc\u{e9})", "perfect", false),
            ("Perfect (ft. Someone)", "perfect", false),
            ("Perfect [featuring Someone]", "perfect", false),
            ("Perfect - feat. Someone", "perfect", false),
            ("Perfect - ft. Someone", "perfect", false),
            ("Perfect (feat. Someone) (Acoustic)", "perfect", true),
            ("  Beyonc\u{e9}   KNOWLES ", "beyonce knowles", false),
            ("Don't Stop", "dont stop", false),
            ("Don\u{2019}t Stop", "dont stop", false),
        ] {
            let title = Title::parse(raw);
            assert_eq!(
                (title.key.as_str(), title.is_qualified()),
                (base, qualified),
                "{raw}"
            );
        }
    }

    #[test]
    fn parts_of_a_title_that_are_not_qualifiers_stay() {
        for (raw, base) in [
            (
                "(I Can't Get No) Satisfaction",
                "i cant get no satisfaction",
            ),
            ("Song (Part 2)", "song part 2"),
            ("Mr. Brightside - Part 2", "mr brightside part 2"),
            ("Live Forever", "live forever"),
            ("Song (2011)", "song 2011"),
            ("Perfect Duet", "perfect duet"),
            ("(Live)", "live"),
            ("Trip to Ft. Lauderdale", "trip to ft lauderdale"),
            ("Perfect feat. Beyonc\u{e9}", "perfect feat beyonce"),
            ("Perfect ft. Someone", "perfect ft someone"),
            ("Song (With You)", "song with you"),
            ("Perfect (with Someone)", "perfect with someone"),
            ("Song - with You", "song with you"),
        ] {
            assert_eq!(Title::parse(raw).key, base, "{raw}");
        }
        assert_eq!(Title::parse("Song - Live Forever").key, "song live forever");
    }

    #[test]
    fn the_base_text_keeps_the_wording_of_the_title() {
        assert_eq!(
            Title::parse("Don't Stop (Live) [Remastered]").text,
            "Don't Stop"
        );
        assert_eq!(Title::parse("Don't Stop").text, "Don't Stop");
    }

    #[test]
    fn plain_titles_match_by_base_within_two_seconds() {
        assert!(matches("Perfect", "perfect", 2000));
        assert!(!matches("Perfect", "Perfect", 2001));
        assert!(matches("Perfect (feat. Someone)", "Perfect", 2000));
        assert!(matches("Perfect", "Perfect - feat. Someone", 0));
        assert!(!matches("Perfect", "Perfect Duet", 0));
        assert!(!matches("Perfect Duet", "Perfect", 0));
        assert!(!matches("Photograph", "Photographs", 0));
        assert!(!matches("Photographs", "Photograph", 0));
        assert!(!matches("Perfect", "Perfect - Live", 0));
        assert!(!matches("Perfect", "Perfect (Remix)", 0));
    }

    #[test]
    fn a_qualified_title_matches_the_base_title_within_two_seconds() {
        assert!(matches("Shape of You (Acoustic)", "Shape of You", 2000));
        assert!(!matches("Shape of You (Acoustic)", "Shape of You", 2001));
        assert!(matches(
            "Castle on the Hill [Remastered]",
            "Castle on the Hill",
            500
        ));
        assert!(matches("Song - Radio Edit", "Song", 1999));
        assert!(!matches("Shape of You (Acoustic)", "Shape of You Duet", 0));
    }

    #[test]
    fn the_same_qualifier_kind_widens_the_length_to_ten_seconds() {
        assert!(matches("Perfect - Live", "Perfect (Live)", 10_000));
        assert!(!matches("Perfect - Live", "Perfect (Live)", 10_001));
        assert!(matches("Perfect - Live", "Perfect (Live at Wembley)", 9000));
        assert!(matches("Hello [Remastered]", "Hello (2011 Remaster)", 8000));
        assert!(matches(
            "Shape of You (Acoustic)",
            "Shape of You - Acoustic",
            9000
        ));
        assert!(!matches("Perfect - Live", "Perfect (Acoustic)", 0));
        assert!(!matches("Perfect (Live)", "Perfect (Live) [Remastered]", 0));
    }

    #[test]
    fn a_remix_matches_only_remixes_of_the_same_base_title() {
        assert!(matches(
            "Thinking Out Loud - Alex Ghenea Remix",
            "Thinking Out Loud (Other Remix)",
            9000
        ));
        assert!(!matches(
            "Thinking Out Loud - Alex Ghenea Remix",
            "Thinking Out Loud (Other Remix)",
            10_001
        ));
        assert!(!matches(
            "Thinking Out Loud - Alex Ghenea Remix",
            "Thinking Out Loud",
            0
        ));
        assert!(!matches(
            "Thinking Out Loud - Alex Ghenea Remix",
            "Thinking Out Loud (Live)",
            0
        ));
        assert!(!matches(
            "Thinking Out Loud - Alex Ghenea Remix",
            "Photograph (Alex Ghenea Remix)",
            0
        ));
        assert!(!matches(
            "Thinking Out Loud",
            "Thinking Out Loud (Remix)",
            0
        ));
    }
}
