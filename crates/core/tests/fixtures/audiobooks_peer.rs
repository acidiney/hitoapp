use crate::library::{self, Library};
use hito_core::{
    Action, Actor, Browse, Credentials, Error, MetadataHistoryQuery, PlaylistDownloadQuery, Search,
};
use serde_json::Value;
use uuid::Uuid;

pub struct Peer {
    pub actor: Actor,
    pub book: Uuid,
    pub entry: Uuid,
}

impl Peer {
    pub async fn prepare(library: &Library) -> Self {
        let session = library
            .hito
            .identity
            .login(Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let admin = library
            .hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        let user = library
            .hito
            .identity
            .create_user(
                &admin,
                Credentials {
                    username: "book-neighbor".into(),
                    password: "audiobook neighbor password".into(),
                },
            )
            .await
            .unwrap();
        let shared = library
            .hito
            .catalog
            .libraries(&library.actor)
            .await
            .unwrap()
            .into_iter()
            .find(|item| !item.personal)
            .unwrap();
        library
            .hito
            .identity
            .set_library_access(&admin, &shared.id, &user.id, true)
            .await
            .unwrap();
        let session = library
            .hito
            .identity
            .login(Credentials {
                username: "book-neighbor".into(),
                password: "audiobook neighbor password".into(),
            })
            .await
            .unwrap();
        let actor = library
            .hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        assert!(
            !actor.user().admin,
            "Both book owners must use ordinary shared-library access"
        );
        let book = Uuid::new_v4();
        let entry = Uuid::new_v4();
        library::apply(
            &library.hito,
            &actor,
            vec![
                Action::PlaylistCreate {
                    playlist_id: book,
                    name: "Private neighboring book".into(),
                },
                Action::EntryInsert {
                    playlist_id: book,
                    entry_id: entry,
                    recording_id: library.recordings[0],
                    position: 0,
                },
                Action::AudiobookCreate { playlist_id: book },
                Action::RecordingTitleSet {
                    recording_id: library.recordings[0],
                    title: "Private neighboring chapter title".into(),
                },
                Action::RecordingArtistSet {
                    recording_id: library.recordings[0],
                    artist: "Private neighboring narrator".into(),
                },
                Action::LikeSet {
                    recording_id: library.recordings[0],
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id: library.recordings[0],
                    position_ms: 500,
                    speed: 1.75,
                },
            ],
        )
        .await;
        Self { actor, book, entry }
    }

    pub async fn snapshot(&self, library: &Library) -> Value {
        let mut metadata = Vec::new();
        let mut sources = Vec::new();
        for recording in library.recordings {
            metadata.push(
                library
                    .hito
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
                library
                    .hito
                    .catalog
                    .sources(&self.actor, &recording.to_string())
                    .await
                    .unwrap(),
            );
        }
        serde_json::json!({
            "book": library.hito.collection.audiobook(&self.actor, &self.book.to_string()).await.unwrap(),
            "libraries": library.hito.catalog.libraries(&self.actor).await.unwrap(),
            "recordings": library.hito.catalog.recordings(&self.actor, Search::default()).await.unwrap(),
            "artists": library.hito.catalog.artists(&self.actor, Browse::default()).await.unwrap(),
            "releases": library.hito.catalog.releases(&self.actor, Browse::default()).await.unwrap(),
            "metadata": metadata,
            "sources": sources,
            "playlists": library.hito.collection.playlists(&self.actor).await.unwrap(),
            "activity": library.hito.collection.activity(&self.actor).await.unwrap(),
            "progress": library.hito.collection.saved_playback(&self.actor, &library.recordings).await.unwrap(),
            "events": library.hito.collection.events(&self.actor, 0).await.unwrap(),
        })
    }

    pub async fn assert_no_downloads(&self, library: &Library) {
        for recording in library.recordings {
            assert!(
                matches!(
                    library
                        .hito
                        .downloads
                        .status(&self.actor, &recording.to_string())
                        .await,
                    Err(Error::NotFound)
                ),
                "Denied bookmark operations must not create private recording downloads"
            );
        }
        assert!(
            matches!(
                library
                    .hito
                    .downloads
                    .playlist_status(
                        &self.actor,
                        &self.book.to_string(),
                        PlaylistDownloadQuery::default()
                    )
                    .await,
                Err(Error::NotFound)
            ),
            "Book ownership checks must not create a playlist download"
        );
    }
}
