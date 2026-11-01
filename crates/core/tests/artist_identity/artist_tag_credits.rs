use hito_core::test_support::{musicbrainz::FakeMusicBrainz, support};
use hito_core::{
    ArtistAssociationEvidence, Browse, CreditRole, MatchArtistsFromTags, MetadataHistoryQuery,
    Search, Services,
};
use lofty::{
    TextEncoding,
    config::WriteOptions,
    id3::v2::{ExtendedTextFrame, Frame, FrameId, Id3v2Tag, TextInformationFrame},
    prelude::{Accessor, ItemKey, TagExt},
    tag::{ItemValue, Tag, TagItem, TagType},
};
use std::{collections::HashMap, path::Path, process::Command};
use uuid::Uuid;

#[path = "../fixtures/metadata_tags_v2.rs"]
mod prior_inspection;

const NOVA: &str = "11111111-1111-4111-8111-111111111111";
const GUEST: &str = "22222222-2222-4222-8222-222222222222";
const ADA: &str = "33333333-3333-4333-8333-333333333333";
const BO: &str = "44444444-4444-4444-8444-444444444444";

struct Song<'a> {
    name: &'a str,
    extension: &'a str,
    artist: &'a str,
    artists: &'a [&'a str],
    ids: &'a [&'a str],
}

fn write(directory: &Path, music: &Path, offset: i16, song: &Song) {
    let source = directory.join(format!("{}.source.wav", song.name));
    support::write_wav(&source, offset);
    let (encoder, kind): (&[&str], TagType) = match song.extension {
        "flac" => (&["-c:a", "flac"], TagType::VorbisComments),
        "opus" => (&["-c:a", "libopus"], TagType::VorbisComments),
        "mp3" => (
            &[
                "-c:a",
                "libmp3lame",
                "-write_id3v2",
                "0",
                "-write_xing",
                "0",
            ],
            TagType::Id3v2,
        ),
        _ => (&["-c:a", "aac"], TagType::Mp4Ilst),
    };
    let path = music.join(format!("{}.{}", song.name, song.extension));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(encoder)
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    if kind == TagType::Id3v2 {
        let frames = [
            ("TIT2", song.name.to_owned()),
            ("TPE1", song.artist.to_owned()),
        ];
        let mut id3 = Id3v2Tag::new();
        for (id, text) in frames {
            id3.insert(Frame::Text(TextInformationFrame::new(
                FrameId::Valid(id.into()),
                TextEncoding::UTF8,
                text,
            )));
        }
        for (description, values) in [
            ("ARTISTS", song.artists),
            ("MusicBrainz Artist Id", song.ids),
        ] {
            if !values.is_empty() {
                id3.insert(Frame::UserText(ExtendedTextFrame::new(
                    TextEncoding::UTF8,
                    description.into(),
                    values.join("\0"),
                )));
            }
        }
        id3.save_to_path(&path, WriteOptions::default()).unwrap();
        return;
    }
    let mut tag = Tag::new(kind);
    tag.set_title(song.name.into());
    tag.set_artist(song.artist.into());
    for name in song.artists {
        tag.push(TagItem::new(
            ItemKey::TrackArtists,
            ItemValue::Text((*name).into()),
        ));
    }
    for id in song.ids {
        tag.push(TagItem::new(
            ItemKey::MusicBrainzArtistId,
            ItemValue::Text((*id).into()),
        ));
    }
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
}

struct World {
    hito: Services,
    actor: hito_core::Actor,
    library: String,
    fake: FakeMusicBrainz,
    ids: HashMap<String, String>,
    directory: tempfile::TempDir,
}

impl World {
    async fn prepare(songs: &[Song<'_>]) -> Self {
        let fake = FakeMusicBrainz::start().await;
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz("tag-credits", Some(fake.endpoint.clone())).await;
        for (index, song) in songs.iter().enumerate() {
            write(directory.path(), &music, (index as i16 + 1) * 700, song);
        }
        support::scan(&hito, &actor, &library).await;
        let mut world = Self {
            hito,
            actor,
            library,
            fake,
            ids: HashMap::new(),
            directory,
        };
        world.refresh_ids().await;
        world
    }

    async fn refresh_ids(&mut self) {
        self.ids = self
            .hito
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|item| (item.title, item.id))
            .collect();
    }

    async fn credited(&self, name: &str) -> Vec<(String, CreditRole)> {
        self.hito
            .catalog
            .recording_metadata(
                &self.actor,
                &self.ids[name],
                MetadataHistoryQuery::default(),
            )
            .await
            .unwrap()
            .credited_artists
            .into_iter()
            .map(|artist| (artist.name, artist.role))
            .collect()
    }

    async fn artists(&self) -> Vec<(String, i64)> {
        let mut artists: Vec<_> = self
            .hito
            .catalog
            .artists(&self.actor, Browse::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|artist| (artist.name, artist.recording_count))
            .filter(|(_, count)| *count > 0)
            .collect();
        artists.sort();
        artists
    }
}

fn main(name: &str) -> (String, CreditRole) {
    (name.to_owned(), CreditRole::Main)
}

fn featured(name: &str) -> (String, CreditRole) {
    (name.to_owned(), CreditRole::Featured)
}

#[tokio::test]
async fn repeated_artist_tags_credit_the_main_artists_in_every_container_whatever_their_order() {
    let world = World::prepare(&[
        Song {
            name: "flac",
            extension: "flac",
            artist: "Ada & Bo",
            artists: &["Ada", "Bo"],
            ids: &[],
        },
        Song {
            name: "opus",
            extension: "opus",
            artist: "Bo, Ada",
            artists: &["Bo", "Ada"],
            ids: &[],
        },
        Song {
            name: "mp3",
            extension: "mp3",
            artist: "Ada and Bo",
            artists: &["Ada", "Bo"],
            ids: &[],
        },
        Song {
            name: "m4a",
            extension: "m4a",
            artist: "BO & ADA",
            artists: &["BO", "ADA"],
            ids: &[],
        },
        Song {
            name: "nulsep",
            extension: "mp3",
            artist: "Ada\0Bo",
            artists: &[],
            ids: &[],
        },
        Song {
            name: "text",
            extension: "flac",
            artist: "Ada and Bo",
            artists: &[],
            ids: &[],
        },
        Song {
            name: "solo",
            extension: "flac",
            artist: "Solo",
            artists: &["Solo"],
            ids: &[],
        },
    ])
    .await;
    assert_eq!(world.credited("flac").await, [main("Ada"), main("Bo")]);
    assert_eq!(world.credited("opus").await, [main("Bo"), main("Ada")]);
    assert_eq!(world.credited("mp3").await, [main("Ada"), main("Bo")]);
    assert_eq!(world.credited("m4a").await, [main("BO"), main("ADA")]);
    assert_eq!(world.credited("nulsep").await, [main("Ada"), main("Bo")]);
    assert!(world.credited("text").await.is_empty());
    assert!(world.credited("solo").await.is_empty());
    let artists = world.artists().await;
    assert_eq!(artists.len(), 3, "{artists:?}");
    assert!(
        artists
            .iter()
            .any(|(name, count)| name.contains(" & ") && *count == 5),
        "The five tagged files share one credit: {artists:?}"
    );
    assert!(artists.contains(&("Ada and Bo".to_owned(), 1)));
    assert!(artists.contains(&("Solo".to_owned(), 1)));
    assert!(world.fake.requests().is_empty(), "Scanning contacts nobody");
}

#[tokio::test]
async fn an_artist_is_featured_only_when_the_display_artist_names_it_in_a_feature_clause() {
    let world = World::prepare(&[
        Song {
            name: "bare",
            extension: "flac",
            artist: "Cy feat. Di",
            artists: &["Cy", "Di"],
            ids: &[],
        },
        Song {
            name: "short",
            extension: "mp3",
            artist: "Cy ft. Di & Ed",
            artists: &["Cy", "Di", "Ed"],
            ids: &[],
        },
        Song {
            name: "bracket",
            extension: "m4a",
            artist: "Cy (featuring Di)",
            artists: &["Cy", "Di"],
            ids: &[],
        },
        Song {
            name: "plain",
            extension: "flac",
            artist: "Cy & Di",
            artists: &["Cy", "Di"],
            ids: &[],
        },
        Song {
            name: "other",
            extension: "flac",
            artist: "Cy feat. Ed",
            artists: &["Cy", "Di"],
            ids: &[],
        },
    ])
    .await;
    assert_eq!(world.credited("bare").await, [main("Cy"), featured("Di")]);
    assert_eq!(
        world.credited("short").await,
        [main("Cy"), featured("Di"), featured("Ed")]
    );
    assert_eq!(
        world.credited("bracket").await,
        [main("Cy"), featured("Di")]
    );
    assert_eq!(world.credited("plain").await, [main("Cy"), main("Di")]);
    assert_eq!(world.credited("other").await, [main("Cy"), main("Di")]);
    let artists = world.artists().await;
    assert!(artists.contains(&("Cy".to_owned(), 3)), "{artists:?}");
}

#[tokio::test]
async fn tagged_artist_ids_identify_a_lone_main_artist_and_a_collaboration_stays_undecided() {
    let world = World::prepare(&[
        Song {
            name: "nova",
            extension: "flac",
            artist: "Nova feat. Guest",
            artists: &["Nova", "Guest"],
            ids: &[NOVA, GUEST],
        },
        Song {
            name: "duo-flac",
            extension: "opus",
            artist: "Ada & Bo",
            artists: &["Ada", "Bo"],
            ids: &[ADA, BO],
        },
        Song {
            name: "duo-mp3",
            extension: "mp3",
            artist: "Ada & Bo",
            artists: &["Ada", "Bo"],
            ids: &[ADA, BO],
        },
        Song {
            name: "duo-m4a",
            extension: "m4a",
            artist: "Ada & Bo",
            artists: &["Ada", "Bo"],
            ids: &[ADA, BO],
        },
    ])
    .await;
    let receipt = world
        .hito
        .artist_catalog
        .match_artists_from_tags(
            &world.actor,
            MatchArtistsFromTags {
                request_id: Uuid::new_v4(),
                after: None,
                limit: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (receipt.matched, receipt.skipped_ambiguous),
        (1, 3),
        "{receipt:?}"
    );
    let linked = world
        .hito
        .artist_catalog
        .recording_artist_associations(&world.actor, &world.ids["nova"])
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].provider_artist_id, NOVA);
    assert_eq!(
        linked[0].evidence,
        ArtistAssociationEvidence::MatchedFileTagArtistId
    );
    for duo in ["duo-flac", "duo-mp3", "duo-m4a"] {
        assert!(
            world
                .hito
                .artist_catalog
                .recording_artist_associations(&world.actor, &world.ids[duo])
                .await
                .unwrap()
                .is_empty(),
            "{duo}"
        );
    }
    assert!(world.fake.requests().is_empty());
}

#[tokio::test]
async fn files_inspected_before_version_eight_gain_their_credits_on_the_next_scan() {
    let mut world = World::prepare(&[Song {
        name: "old",
        extension: "mp3",
        artist: "Ada & Bo",
        artists: &["Ada", "Bo"],
        ids: &[],
    }])
    .await;
    let database = world.directory.path().join("hito.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", database.display()))
        .await
        .unwrap();
    let source: String = sqlx::query_scalar("SELECT source_id FROM source_metadata")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    prior_inspection::represent_prior_inspection(&database, &source).await;
    while world
        .hito
        .artist_catalog
        .run_next_automatic_matching_until(std::future::pending())
        .await
        .unwrap()
    {}
    support::scan(&world.hito, &world.actor, &world.library).await;
    world.refresh_ids().await;
    assert_eq!(world.credited("old").await, [main("Ada"), main("Bo")]);
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", database.display()))
        .await
        .unwrap();
    let due: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM artist_matching_jobs WHERE next_run_at <= unixepoch()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    pool.close().await;
    assert_eq!(due, 1, "The scan alone queues the account's matching walk");
}

#[tokio::test]
async fn a_file_with_thousands_of_artist_tags_keeps_its_text_credit_and_stays_playable() {
    let names: Vec<String> = (0..3000)
        .map(|index| format!("Crowd artist {index}"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let world = World::prepare(&[Song {
        name: "crowd",
        extension: "flac",
        artist: "Crowd",
        artists: &refs,
        ids: &[],
    }])
    .await;
    assert!(world.credited("crowd").await.is_empty());
    assert_eq!(world.artists().await, [("Crowd".to_owned(), 1)]);
}
