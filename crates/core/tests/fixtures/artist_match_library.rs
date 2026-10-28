use hito_core::test_support::{playlist_service as provider, support};
use hito_core::{
    Actor, ArtistMatchReceipt, Browse, Credentials, ImportProviderPlaylist, MatchArtistsFromTags,
    MetadataHistoryQuery, Services,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use sqlx::Connection;
use std::path::PathBuf;
use uuid::Uuid;

pub const CREDIT: &str = "C4 Pedro";
pub const SPELLED: &str = "C4 Pedro & Co";
pub const M1: &str = "11111111-1111-4111-8111-111111111111";
pub const M2: &str = "22222222-2222-4222-8222-222222222222";

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub peer: Actor,
    pub library: String,
    pub a: String,
    pub b: String,
    pub namesake: String,
    pub plain: String,
    pub multi: String,
    pub imported: String,
    pub origin: provider::ProviderServer,
    pub music: PathBuf,
    _directory: tempfile::TempDir,
}

pub fn tagged(
    path: &std::path::Path,
    offset: i16,
    (title, album): (&str, &str),
    artist: &str,
    mbid: Option<&str>,
) {
    support::write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    if let Some(mbid) = mbid {
        tag.insert_text(ItemKey::MusicBrainzArtistId, mbid.into());
    }
    tag.save_to_path(path, Default::default()).unwrap();
}

impl Library {
    pub async fn prepare() -> Self {
        let (directory, hito, actor, library, music) = support::library("artist-match").await;
        tagged(
            &music.join("a.wav"),
            1,
            ("Song A", "Album A"),
            CREDIT,
            Some(M1),
        );
        tagged(
            &music.join("b.wav"),
            2,
            ("Song B", "Album B"),
            SPELLED,
            Some(M1),
        );
        tagged(
            &music.join("n.wav"),
            3,
            ("Song N", "Album N"),
            CREDIT,
            Some(M2),
        );
        tagged(
            &music.join("p.wav"),
            4,
            ("song a ", "Album A"),
            CREDIT,
            None,
        );
        tagged(
            &music.join("m.wav"),
            5,
            ("Song M", "Album M"),
            "Duo",
            Some(&format!("{M1};{M2}")),
        );
        support::scan(&hito, &actor, &library).await;
        let mut ids = std::collections::HashMap::new();
        for item in hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
        {
            ids.insert(item.title.clone(), item.id.clone());
        }
        let user = hito
            .identity
            .create_user(
                &actor,
                Credentials {
                    username: "peer".into(),
                    password: "owned matching peer password".into(),
                },
            )
            .await
            .unwrap();
        hito.identity
            .set_library_access(&actor, &library, &user.id, true)
            .await
            .unwrap();
        let session = hito
            .identity
            .login(Credentials {
                username: "peer".into(),
                password: "owned matching peer password".into(),
            })
            .await
            .unwrap();
        let peer = hito.identity.authenticate(&session.token).await.unwrap();
        let origin = provider::ProviderServer::start().await;
        let installation = provider::connect(&hito, &actor, &origin, &["playlists"]).await;
        provider::playlist(
            &origin,
            "owned-match",
            "owned-match-v1",
            vec![
                json!({"occurrence_id": "only", "item": {"kind": "recording", "recording": {
                    "id": "I", "title": "Imported Song", "artist": CREDIT, "duration_ms": 1000,
                    "release": null, "disc_number": null, "track_number": null, "revision": "v1"
                }}}),
            ],
        )
        .await;
        let receipt = hito
            .collection
            .import_provider_playlist(
                &actor,
                &installation.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installation.revision,
                    playlist_reference: "https://provider.invalid/owned-match".into(),
                },
            )
            .await
            .unwrap();
        let playlist = hito
            .collection
            .playlist(&actor, &receipt.playlist_id.to_string())
            .await
            .unwrap();
        let imported = playlist.entries[0].recording_id.clone().unwrap();
        Self {
            a: ids["Song A"].clone(),
            b: ids["Song B"].clone(),
            namesake: ids["Song N"].clone(),
            plain: ids["song a "].clone(),
            multi: ids["Song M"].clone(),
            imported,
            hito,
            actor,
            peer,
            library,
            origin,
            music,
            _directory: directory,
        }
    }

    pub async fn rows(&self, table: &str) -> i64 {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=ro",
            self._directory.path().join("hito.db").display()
        ))
        .await
        .unwrap();
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut connection)
            .await
            .unwrap()
    }

    pub fn all(&self) -> [&String; 6] {
        [
            &self.a,
            &self.b,
            &self.namesake,
            &self.plain,
            &self.multi,
            &self.imported,
        ]
    }

    pub async fn run(&self) -> ArtistMatchReceipt {
        self.run_as(&self.actor).await
    }

    pub async fn run_as(&self, actor: &Actor) -> ArtistMatchReceipt {
        self.hito
            .catalog
            .match_artists_from_tags(
                actor,
                MatchArtistsFromTags {
                    request_id: Uuid::new_v4(),
                    after: None,
                    limit: None,
                },
            )
            .await
            .unwrap()
    }

    pub async fn credit(&self, name: &str) -> Option<hito_core::Artist> {
        self.hito
            .catalog
            .artists(
                &self.actor,
                Browse {
                    q: Some(name.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|artist| artist.name == name && artist.library_id == self.library)
    }

    pub async fn snapshot(&self) -> Value {
        let mut recordings = Vec::new();
        for id in self.all() {
            recordings.push(json!({
                "metadata": self.hito.catalog.recording_metadata(&self.actor, id, MetadataHistoryQuery::default()).await.unwrap(),
                "sources": self.hito.catalog.sources(&self.actor, id).await.unwrap(),
            }));
        }
        json!({
            "recordings": self.hito.catalog.recordings(&self.actor, Default::default()).await.unwrap(),
            "releases": self.hito.catalog.releases(&self.actor, Default::default()).await.unwrap(),
            "detail": recordings,
            "activity": self.hito.collection.activity(&self.actor).await.unwrap(),
        })
    }
}
