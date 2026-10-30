use super::support;
use hito_core::{
    Action, Actor, Browse, CoreConfig, Credentials, Error, HostDeviceId, MetadataHistoryQuery,
    Operation, PlaylistDownloadQuery, Search, Services, SyncBatch,
};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use serde_json::Value;
use std::path::PathBuf;
use uuid::Uuid;

pub const BOOK_TITLE: &str = "Private ordered audiobook";
pub const PERSONAL_TITLE: &str = "Private chapter title";
pub const PERSONAL_ARTIST: &str = "Private narrator credit";

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub recordings: [Uuid; 2],
    pub playlist: Uuid,
    pub entries: [Uuid; 2],
    unrelated_playlist: Uuid,
    directory: tempfile::TempDir,
    music: PathBuf,
    originals: Vec<(PathBuf, Vec<u8>)>,
    token: String,
}

impl Library {
    pub async fn prepare() -> Self {
        let (directory, hito, admin, library, music) = support::library("audiobook-host").await;
        let mut originals = Vec::new();
        for (offset, title) in ["Observed Alpha chapter", "Observed Beta chapter"]
            .into_iter()
            .enumerate()
        {
            let path = music.join(format!("chapter-{offset}.wav"));
            support::write_wav(&path, offset as i16);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title(title.into());
            tag.set_artist("Observed narrator".into());
            tag.set_album("Observed chapter collection".into());
            tag.save_to_path(&path, WriteOptions::default()).unwrap();
            originals.push((path.clone(), std::fs::read(path).unwrap()));
        }
        support::scan(&hito, &admin, &library).await;
        let user = hito
            .identity
            .create_user(
                &admin,
                Credentials {
                    username: "book-listener".into(),
                    password: "audiobook listener password".into(),
                },
            )
            .await
            .unwrap();
        hito.identity
            .set_library_access(&admin, &library, &user.id, true)
            .await
            .unwrap();
        let session = hito
            .identity
            .login(Credentials {
                username: "book-listener".into(),
                password: "audiobook listener password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        assert!(
            !actor.user().admin,
            "The book owner must use an ordinary library grant"
        );
        let catalog = hito
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert!(
            catalog.total == 2 && catalog.items.len() == 2,
            "The owned fixture must index exactly two distinct original recordings"
        );
        let recordings = [
            Uuid::parse_str(&catalog.items[1].id).unwrap(),
            Uuid::parse_str(&catalog.items[0].id).unwrap(),
        ];
        assert!(
            recordings[0] != recordings[1],
            "The chapter recordings must remain distinct"
        );
        let playlist = Uuid::new_v4();
        let entries = [Uuid::new_v4(), Uuid::new_v4()];
        let unrelated_playlist = Uuid::new_v4();
        let mut actions = vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: BOOK_TITLE.into(),
            },
            Action::PlaylistCreate {
                playlist_id: unrelated_playlist,
                name: "Private unrelated duplicate selection".into(),
            },
        ];
        for index in 0..2 {
            actions.push(Action::EntryInsert {
                playlist_id: playlist,
                entry_id: entries[index],
                recording_id: recordings[index],
                position: (index as i64 + 1) * 10,
            });
            actions.push(Action::EntryInsert {
                playlist_id: unrelated_playlist,
                entry_id: Uuid::new_v4(),
                recording_id: recordings[0],
                position: index as i64,
            });
        }
        actions.extend([
            Action::RecordingTitleSet {
                recording_id: recordings[0],
                title: PERSONAL_TITLE.into(),
            },
            Action::RecordingArtistSet {
                recording_id: recordings[0],
                artist: PERSONAL_ARTIST.into(),
            },
            Action::LikeSet {
                recording_id: recordings[1],
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recordings[0],
                position_ms: 100,
                speed: 1.5,
            },
            Action::ProgressSet {
                recording_id: recordings[1],
                position_ms: 650,
                speed: 1.25,
            },
        ]);
        apply(&hito, &actor, actions).await;
        Self {
            hito,
            actor,
            recordings,
            playlist,
            entries,
            unrelated_playlist,
            directory,
            music,
            originals,
            token: session.token,
        }
    }

    pub async fn reopen(self) -> Self {
        let Self {
            hito,
            actor,
            recordings,
            playlist,
            entries,
            unrelated_playlist,
            directory,
            music,
            originals,
            token,
        } = self;
        drop(actor);
        drop(hito);
        let hito = hito_core::open(CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("audiobook-host").unwrap(),
        ))
        .await
        .unwrap();
        let actor = hito.identity.authenticate(&token).await.unwrap();
        Self {
            hito,
            actor,
            recordings,
            playlist,
            entries,
            unrelated_playlist,
            directory,
            music,
            originals,
            token,
        }
    }

    pub async fn snapshot(&self) -> Value {
        let mut metadata = Vec::new();
        let mut sources = Vec::new();
        for recording in self.recordings {
            metadata.push(
                self.hito
                    .catalog
                    .recording_metadata(
                        &self.actor,
                        &recording.to_string(),
                        MetadataHistoryQuery::default(),
                    )
                    .await
                    .unwrap(),
            );
            sources.push(
                self.hito
                    .catalog
                    .sources(&self.actor, &recording.to_string())
                    .await
                    .unwrap(),
            );
        }
        serde_json::json!({
            "libraries": self.hito.catalog.libraries(&self.actor).await.unwrap(),
            "recordings": self.hito.catalog.recordings(&self.actor, Search::default()).await.unwrap(),
            "artists": self.hito.catalog.artists(&self.actor, Browse::default()).await.unwrap(),
            "releases": self.hito.catalog.releases(&self.actor, Browse::default()).await.unwrap(),
            "metadata": metadata,
            "sources": sources,
            "playlists": self.hito.collection.playlists(&self.actor).await.unwrap(),
            "chapters": self.hito.collection.playlist(&self.actor, &self.playlist.to_string()).await.unwrap(),
            "unrelated": self.hito.collection.playlist(&self.actor, &self.unrelated_playlist.to_string()).await.unwrap(),
            "activity": self.hito.collection.activity(&self.actor).await.unwrap(),
            "progress": self.hito.collection.saved_playback(&self.actor, &self.recordings).await.unwrap(),
        })
    }

    pub async fn assert_no_downloads(&self) {
        for recording in self.recordings {
            assert!(
                matches!(
                    self.hito
                        .downloads
                        .status(&self.actor, &recording.to_string())
                        .await,
                    Err(Error::NotFound)
                ),
                "Grouping and bookmarking must not create a recording download"
            );
        }
        for playlist in [self.playlist, self.unrelated_playlist] {
            assert!(
                matches!(
                    self.hito
                        .downloads
                        .playlist_status(
                            &self.actor,
                            &playlist.to_string(),
                            PlaylistDownloadQuery::default()
                        )
                        .await,
                    Err(Error::NotFound)
                ),
                "Grouping and bookmarking must not create playlist retention"
            );
        }
    }

    pub fn assert_originals_unchanged(&self) {
        for (path, bytes) in &self.originals {
            assert!(
                std::fs::read(path).unwrap() == *bytes,
                "Audiobook collection changes must preserve every original byte"
            );
        }
    }
}

pub async fn apply(hito: &Services, actor: &Actor, actions: Vec<Action>) -> Vec<Uuid> {
    let operations: Vec<Operation> = actions
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect();
    let accepted: Vec<Uuid> = operations.iter().map(|operation| operation.id).collect();
    let receipt = hito
        .collection
        .apply(
            actor,
            SyncBatch {
                device_id: "audiobook-owned-client".into(),
                operations,
            },
        )
        .await
        .unwrap();
    assert!(
        receipt.acknowledged == accepted,
        "Every public collection operation must be acknowledged in accepted order"
    );
    accepted
}
