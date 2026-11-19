use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    support,
};
use hito_core::{Actor, Genre, Recording, Services};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use sqlx::Connection;
use std::{collections::HashSet, path::PathBuf, time::Duration};

pub(super) const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub(super) const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub(super) const GROUP: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
pub(super) const RECORDING: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
pub(super) const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

#[derive(Default)]
pub(super) struct Ids<'a> {
    pub(super) artist: Option<&'a str>,
    pub(super) group: Option<&'a str>,
    pub(super) genre: Option<&'a str>,
}

pub(super) struct World {
    pub(super) hito: Services,
    pub(super) actor: Actor,
    pub(super) music: PathBuf,
    pub(super) library: String,
    pub(super) fake: FakeMusicBrainz,
    directory: tempfile::TempDir,
}

impl World {
    pub(super) async fn new(host: &str) -> Self {
        let fake = FakeMusicBrainz::start().await;
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz(host, Some(fake.endpoint.clone())).await;
        Self {
            hito,
            actor,
            music,
            library,
            fake,
            directory,
        }
    }

    pub(super) fn track(&self, offset: i16, title: &str, ids: Ids) {
        let path = self.music.join(format!("{title}.wav"));
        support::write_wav(&path, offset);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.into());
        tag.set_artist("Performer".into());
        tag.set_album(format!("Album {title}"));
        if let Some(genre) = ids.genre {
            tag.set_genre(genre.into());
        }
        for (key, value) in [
            (ItemKey::MusicBrainzArtistId, ids.artist),
            (ItemKey::MusicBrainzReleaseGroupId, ids.group),
        ] {
            if let Some(value) = value {
                tag.insert_text(key, value.into());
            }
        }
        tag.save_to_path(&path, Default::default()).unwrap();
    }

    pub(super) fn flac_track(&self, offset: i16, title: &str, recording: &str) {
        let source = self.directory.path().join(format!("{title}.wav"));
        support::write_wav(&source, offset);
        let path = self.music.join(format!("{title}.flac"));
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(&source)
            .args(["-c:a", "flac"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.set_title(title.into());
        tag.set_artist("Performer".into());
        tag.insert_text(ItemKey::MusicBrainzRecordingId, recording.into());
        tag.save_to_path(&path, Default::default()).unwrap();
    }

    pub(super) fn artist_genres(&self, artist: &str, genres: &[(&str, u32)]) {
        self.fake.genres_of(
            "artist",
            artist,
            Reply::Genres(
                genres
                    .iter()
                    .map(|(name, count)| ((*name).to_owned(), *count))
                    .collect(),
            ),
        );
    }

    pub(super) async fn scan(&self) {
        support::scan(&self.hito, &self.actor, &self.library).await;
    }

    pub(super) async fn walk(&self) -> usize {
        let mut steps = 0;
        while self
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
        {
            steps += 1;
            assert!(steps < 60, "The automatic walk must end");
        }
        steps
    }

    pub(super) fn genre_requests(&self) -> Vec<String> {
        self.fake
            .requests()
            .into_iter()
            .filter(|request| request.contains("inc=genres"))
            .collect()
    }

    pub(super) async fn tiles(&self) -> Vec<(String, i64)> {
        tiles(self.hito.catalog.genres(&self.actor).await.unwrap())
    }

    pub(super) async fn connect(&self) -> sqlx::SqliteConnection {
        sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}",
            self.directory.path().join("hito.db").display()
        ))
        .await
        .unwrap()
    }

    pub(super) async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&mut self.connect().await)
            .await
            .unwrap()
    }

    pub(super) async fn execute(&self, sql: &str) {
        sqlx::query(sql)
            .execute(&mut self.connect().await)
            .await
            .unwrap();
    }
}

pub(super) fn tiles(genres: Vec<Genre>) -> Vec<(String, i64)> {
    genres
        .into_iter()
        .map(|genre| (genre.key, genre.track_count))
        .collect()
}

pub(super) fn pairs(expected: &[(&str, i64)]) -> Vec<(String, i64)> {
    expected
        .iter()
        .map(|(key, count)| ((*key).to_owned(), *count))
        .collect()
}

pub(super) async fn two_artists() -> World {
    let world = World::new("mb-genres").await;
    for (offset, title, artist) in [(1, "a1", A), (2, "a2", A), (3, "b1", B)] {
        world.track(
            offset,
            title,
            Ids {
                artist: Some(artist),
                ..Ids::default()
            },
        );
    }
    world.track(4, "plain", Ids::default());
    world.artist_genres(A, &[("fado", 5), ("folk", 1)]);
    world.artist_genres(B, &[("jazz", 3)]);
    world.scan().await;
    world
}

pub(super) async fn paged_tracks_match_preview(world: &World, key: &str) -> Vec<Recording> {
    let preview = tokio::time::timeout(
        Duration::from_secs(5),
        world.hito.catalog.genre(&world.actor, key),
    )
    .await
    .expect("genre preview finishes within 5 seconds")
    .unwrap();
    let expected_count = preview.tracks.len();
    assert!(expected_count > 0 && expected_count <= 100);
    let mut tracks = Vec::with_capacity(expected_count);
    let mut recording_ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut after = None;
    for index in 0..expected_count {
        let page = tokio::time::timeout(
            Duration::from_secs(5),
            world
                .hito
                .catalog
                .genre_tracks(&world.actor, key, Some(1), after.as_deref()),
        )
        .await
        .expect("genre track page finishes within 5 seconds")
        .unwrap();
        assert_eq!(page.items.len(), 1);
        let track = page.items.into_iter().next().unwrap();
        assert!(recording_ids.insert(track.id.clone()));
        tracks.push(track);
        let expected_more = index + 1 < expected_count;
        assert_eq!(page.next_after.is_some(), expected_more);
        if let Some(next_after) = page.next_after {
            assert!(!next_after.is_empty());
            assert!(cursors.insert(next_after.clone()));
            after = Some(next_after);
        }
    }
    assert_eq!(
        serde_json::to_value(&tracks).unwrap(),
        serde_json::to_value(&preview.tracks).unwrap()
    );
    tracks
}
