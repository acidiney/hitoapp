use crate::{Error, Result};
use url::Url;

mod snapshot;
mod xml;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct FeedSnapshot {
    pub title: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub episodes: Vec<FeedEpisode>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct FeedEpisode {
    pub identity: EpisodeIdentity,
    pub title: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub enclosure_url: String,
    pub declared_mime: Option<String>,
    pub length: Option<u64>,
    pub duration_ms: Option<u64>,
    pub published_at: Option<i64>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) enum EpisodeIdentity {
    Guid(String),
    Enclosure(String),
}

pub(super) fn parse(bytes: &[u8], base_url: &Url) -> Result<FeedSnapshot> {
    let started = std::time::Instant::now();
    let result = xml::read(bytes).and_then(|root| snapshot::read(&root, base_url));
    tracing::debug!(
        event = "podcast_feed_parsed",
        outcome = if result.is_ok() { "parsed" } else { "invalid" },
        episode_count = result
            .as_ref()
            .map_or(0, |snapshot| snapshot.episodes.len()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Podcast feed parsing finished"
    );
    result
}

fn invalid() -> Error {
    Error::Upstream("The podcast feed is invalid or exceeds its limits")
}

#[cfg(test)]
mod tests {
    use super::{EpisodeIdentity, parse};
    use url::Url;

    #[test]
    fn namespaced_attributes_require_declared_unique_expanded_names() {
        let base = Url::parse("https://feeds.example/owned.xml").unwrap();
        let owned_feed = |extension: &str| {
            format!(
                r#"<rss version="2.0"><channel>
            <title>Owned feed</title>
            <item>
                <guid isPermaLink="false">owned-episode</guid>
                <title>Owned episode</title>
                <enclosure url="https://media.example/owned.mp3" type="audio/mpeg"/>
            </item>
            {extension}
        </channel></rss>"#
            )
        };
        for invalid in [
            r#"<extra undeclared:flag="x"/>"#,
            r#"<extra xmlns:a="urn:x" xmlns:b="urn:x" a:flag="x" b:flag="y"/>"#,
        ] {
            let xml = owned_feed(invalid);
            assert!(
                parse(xml.as_bytes(), &base).is_err(),
                "undeclared or duplicate expanded attribute names must reject the whole snapshot"
            );
        }
        let control = owned_feed(
            r#"<extra xmlns:a="urn:x" xmlns:b="urn:y" xmlns:c="urn:x"
            a:flag="x" b:flag="y" c:other="z" flag="plain"/>"#,
        );
        let snapshot = parse(control.as_bytes(), &base)
            .expect("distinct expanded attribute names and declared namespaces must remain valid");
        assert_eq!(snapshot.episodes.len(), 1);
    }

    #[test]
    fn external_and_undeclared_entities_reject_complete_feed_snapshots() {
        let base = Url::parse("https://feeds.example/owned.xml").unwrap();
        let owned_feed = |prefix: &str, suffix: &str| {
            format!(
                r#"{prefix}
            <rss version="2.0"><channel>
                <title>Owned feed</title>
                <item>
                    <guid isPermaLink="false">owned-episode</guid>
                    <title>Owned episode</title>
                    <enclosure url="https://media.example/owned.mp3" type="audio/mpeg"/>
                </item>
                {suffix}
            </channel></rss>"#
            )
        };
        for (prefix, suffix) in [
            (
                "",
                r#"<!DOCTYPE rss SYSTEM "https://entities.example/owned.dtd">"#,
            ),
            (
                r#"<!DOCTYPE rss [<!ENTITY external SYSTEM "https://entities.example/owned.txt">]>"#,
                "<description>&external;</description>",
            ),
            ("", "<description>&undeclared;</description>"),
        ] {
            let xml = owned_feed(prefix, suffix);
            assert!(
                parse(xml.as_bytes(), &base).is_err(),
                "forbidden declarations or unknown references must reject the whole snapshot"
            );
        }
        let control = owned_feed("", "<description>Owned &amp; &#65;&#x42;</description>");
        let snapshot = parse(control.as_bytes(), &base)
            .expect("predefined and numeric XML references must remain valid");
        assert_eq!(snapshot.episodes.len(), 1);
        assert_eq!(snapshot.description.as_deref(), Some("Owned & AB"));
    }

    #[test]
    fn malformed_xml_names_reject_the_complete_feed_snapshot() {
        let base = Url::parse("https://feeds.example/owned.xml").unwrap();
        for invalid in ["<1illegal/>", "<extra 1illegal=\"value\"/>"] {
            let xml = format!(
                r#"<rss version="2.0"><channel>
                <title>Owned feed</title>
                <description>Owned description</description>
                {invalid}
                <item>
                    <guid isPermaLink="false">owned-episode</guid>
                    <title>Owned episode</title>
                    <enclosure url="https://media.example/owned.mp3" type="audio/mpeg"/>
                </item>
            </channel></rss>"#
            );
            assert!(
                parse(xml.as_bytes(), &base).is_err(),
                "malformed element and attribute names must reject the complete feed snapshot"
            );
        }
        let unicode = r#"<rss version="2.0"><channel>
            <title>Owned Unicode extensions</title>
            <épisode 元="owned"/>
            <音:項目 xmlns:音="urn:owned" 音:識別子="owned"/>
            <item>
                <guid isPermaLink="false">owned-episode</guid>
                <title>Owned episode</title>
                <enclosure url="https://media.example/owned.mp3" type="audio/mpeg"/>
            </item>
        </channel></rss>"#;
        let snapshot = parse(unicode.as_bytes(), &base)
            .expect("legitimate Unicode XML names must remain accepted");
        assert_eq!(snapshot.episodes.len(), 1);
    }

    #[test]
    fn empty_rss_channel_accepts_empty_optional_description_and_author() {
        let xml = br#"<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
            <channel>
                <title>Empty owned feed</title>
                <link>https://publisher.example/</link>
                <description/>
                <itunes:author>
	 </itunes:author>
            </channel>
        </rss>"#;
        let base = Url::parse("https://feeds.example/final/empty.xml").unwrap();
        let snapshot = parse(xml, &base).expect("a valid empty channel must remain a subscription");
        assert_eq!(snapshot.title, "Empty owned feed");
        assert_eq!(snapshot.author, None);
        assert_eq!(snapshot.description, None);
        assert!(snapshot.episodes.is_empty());
    }

    #[test]
    fn rss_snapshot_preserves_opaque_guids_and_resolves_enclosure_fallbacks() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
              <channel>
                <title>Owned &amp; independent</title>
                <link>https://publisher.example/</link>
                <description>Owned &amp; explicit feed description</description>
                <itunes:author>Owned Studio</itunes:author>
                <item>
                  <guid isPermaLink="false">publisher opaque &amp; identity #1</guid>
                  <title>First episode</title>
                  <itunes:author>Owned Guest</itunes:author>
                  <description><![CDATA[<p>Owned & raw description</p>]]></description>
                  <itunes:duration>01:02:03</itunes:duration>
                  <enclosure url="https://Media.example:443/audio/One.mp3?b=2&amp;a=1"
                             type="audio/mpeg" length="123456"/>
                </item>
                <item>
                  <title>Second episode</title>
                  <author>guest@example.invalid (Guest Two)</author>
                  <itunes:duration>95</itunes:duration>
                  <enclosure url="../audio/episode-two.ogg?b=2&amp;a=1"/>
                </item>
                <item>
                  <guid>not-an-audio-episode</guid>
                  <title>Owned cover image</title>
                  <enclosure url="https://media.example/cover.png" type="image/png"/>
                </item>
              </channel>
            </rss>"#;
        let base = Url::parse("https://feeds.example/redirected/library/feed.xml").unwrap();
        let snapshot = parse(xml, &base).expect("a complete owned RSS snapshot must parse");
        assert_eq!(snapshot.title, "Owned & independent");
        assert_eq!(snapshot.author.as_deref(), Some("Owned Studio"));
        assert_eq!(
            snapshot.description.as_deref(),
            Some("Owned & explicit feed description")
        );
        assert_eq!(
            snapshot.episodes.len(),
            2,
            "non-audio enclosures must not become episodes"
        );
        let first = &snapshot.episodes[0];
        assert_eq!(
            first.identity,
            EpisodeIdentity::Guid("publisher opaque & identity #1".into())
        );
        assert_eq!(first.title, "First episode");
        assert_eq!(first.author.as_deref(), Some("Owned Guest"));
        assert_eq!(
            first.description.as_deref(),
            Some("<p>Owned & raw description</p>")
        );
        assert_eq!(
            first.enclosure_url,
            "https://Media.example:443/audio/One.mp3?b=2&a=1"
        );
        assert_eq!(first.declared_mime.as_deref(), Some("audio/mpeg"));
        assert_eq!(first.length, Some(123456));
        assert_eq!(first.duration_ms, Some(3_723_000));
        assert_eq!(first.published_at, None);
        let second = &snapshot.episodes[1];
        let resolved = "https://feeds.example/redirected/audio/episode-two.ogg?b=2&a=1";
        assert_eq!(second.identity, EpisodeIdentity::Enclosure(resolved.into()));
        assert_eq!(second.title, "Second episode");
        assert_eq!(
            second.author.as_deref(),
            Some("guest@example.invalid (Guest Two)")
        );
        assert_eq!(second.description, None);
        assert_eq!(second.enclosure_url, resolved);
        assert_eq!(second.declared_mime, None);
        assert_eq!(second.length, None);
        assert_eq!(second.duration_ms, Some(95_000));
        assert_eq!(second.published_at, None);
    }
}
