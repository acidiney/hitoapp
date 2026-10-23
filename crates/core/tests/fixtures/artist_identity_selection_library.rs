use hito_core::test_support::playlist_service as provider;

use crate::{catalog, support};
use hito_core::{
    Action, Actor, Credentials, Error, ImportProviderPlaylist, InstallPlugin, MetadataProvider,
    OfferPluginService, PlaylistImportReceipt, Services, UpdateMetadataSettings,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub peer: Actor,
    pub shared: String,
    pub local: String,
    pub imported: String,
    pub namesake: String,
    pub receipt: PlaylistImportReceipt,
    pub credit_ids: Vec<String>,
    pub metadata_provider: MetadataProvider,
    pub origin: provider::ProviderServer,
    pub catalog: catalog::CatalogServer,
    audio: PathBuf,
    original: Vec<u8>,
    _directory: tempfile::TempDir,
}

impl Library {
    pub async fn prepare() -> Self {
        let (directory, hito, actor, shared, music) =
            support::library("artist-identity-selection").await;
        let audio = music.join("owned.wav");
        support::write_wav(&audio, 0);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Owned local recording".into());
        tag.set_artist(catalog::CREDIT.into());
        tag.set_album("Owned local album".into());
        tag.insert_text(ItemKey::AlbumArtist, "Local album credit".into());
        tag.save_to_path(&audio, Default::default()).unwrap();
        let original = std::fs::read(&audio).unwrap();
        support::scan(&hito, &actor, &shared).await;
        let local = hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let user = hito
            .identity
            .create_user(
                &actor,
                Credentials {
                    username: "peer".into(),
                    password: "owned identity peer password".into(),
                },
            )
            .await
            .unwrap();
        hito.identity
            .set_library_access(&actor, &shared, &user.id, true)
            .await
            .unwrap();
        let session = hito
            .identity
            .login(Credentials {
                username: "peer".into(),
                password: "owned identity peer password".into(),
            })
            .await
            .unwrap();
        let peer = hito.identity.authenticate(&session.token).await.unwrap();
        let origin = provider::ProviderServer::start().await;
        let installation = provider::connect(&hito, &actor, &origin, &["playlists"]).await;
        let entry = |id: &str, occurrence: &str| {
            json!({
                "occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
                    "id": id, "title": format!("Owned imported {id}"), "artist": catalog::CREDIT,
                    "duration_ms": 1000, "release": if id == "A" { json!({
                        "id": "owned-album-A", "title": "Owned imported album", "artist": "Imported album credit"
                    }) } else { Value::Null }, "disc_number": null, "track_number": null, "revision": "owned-v1"
                }}
            })
        };
        provider::playlist(
            &origin,
            "owned-selection",
            "owned-selection-v1",
            vec![
                entry("A", "first-A"),
                entry("B", "B"),
                entry("A", "second-A"),
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
                    playlist_reference: "https://provider.invalid/owned-selection".into(),
                },
            )
            .await
            .unwrap();
        let playlist = hito
            .collection
            .playlist(&actor, &receipt.playlist_id.to_string())
            .await
            .unwrap();
        assert_eq!(playlist.entries.len(), 3);
        let imported = playlist.entries[0].recording_id.clone().unwrap();
        let namesake = playlist.entries[1].recording_id.clone().unwrap();
        assert_ne!(imported, namesake);
        assert_eq!(
            playlist.entries[2].recording_id.as_deref(),
            Some(imported.as_str())
        );
        assert_ne!(playlist.entries[0].entry_id, playlist.entries[2].entry_id);
        let mut actions = [&local, &imported, &namesake]
            .map(|recording| Action::RecordingArtistSet {
                recording_id: Uuid::parse_str(recording).unwrap(),
                artist: catalog::CREDIT.into(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let id = Uuid::parse_str(&imported).unwrap();
        actions.extend([
            Action::RecordingTitleSet {
                recording_id: id,
                title: "Private imported title".into(),
            },
            Action::LikeSet {
                recording_id: id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: id,
                position_ms: 400,
                speed: 1.25,
            },
        ]);
        support::synchronize(&hito, &actor, actions).await;
        let credit_ids = hito
            .catalog
            .artists(&actor, Default::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|artist| artist.id)
            .collect();
        let catalog = catalog::CatalogServer::start().await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Owned artist identities".into(),
                    base_url: catalog.url.clone(),
                },
            )
            .await
            .unwrap();
        let connection = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let metadata_provider = MetadataProvider::HttpPlugin {
            installation_id: connection.id,
        };
        let settings = hito.catalog.metadata_settings(&actor).await.unwrap();
        hito.catalog
            .set_metadata_settings(
                &actor,
                UpdateMetadataSettings {
                    revision: settings.revision,
                    enabled: true,
                    providers: vec![metadata_provider.clone()],
                },
            )
            .await
            .unwrap();
        Self {
            hito,
            actor,
            peer,
            shared,
            local,
            imported,
            namesake,
            receipt,
            credit_ids,
            metadata_provider,
            origin,
            catalog,
            audio,
            original,
            _directory: directory,
        }
    }

    #[allow(dead_code)]
    pub fn music(&self) -> PathBuf {
        self.audio.parent().unwrap().to_path_buf()
    }

    #[allow(dead_code)]
    pub fn database(&self) -> PathBuf {
        self._directory.path().join("hito.db")
    }

    pub async fn snapshot(&self) -> Value {
        let mut metadata = Vec::new();
        let mut sources = Vec::new();
        for recording in [&self.local, &self.imported, &self.namesake] {
            metadata.push(
                self.hito
                    .catalog
                    .recording_metadata(&self.actor, recording, Default::default())
                    .await
                    .unwrap(),
            );
            let current = self
                .hito
                .catalog
                .sources(&self.actor, recording)
                .await
                .unwrap();
            if recording != &self.local {
                assert!(current.is_empty());
                assert!(matches!(
                    self.hito.downloads.status(&self.actor, recording).await,
                    Err(Error::NotFound)
                ));
            }
            sources.push(current);
        }
        assert_eq!(std::fs::read(&self.audio).unwrap(), self.original);
        let mut credit_pages = Vec::new();
        for id in &self.credit_ids {
            credit_pages.push(
                self.hito
                    .catalog
                    .artist(&self.actor, id, Default::default())
                    .await
                    .map(Some)
                    .or_else(|error| match error {
                        Error::NotFound => Ok(None),
                        other => Err(other),
                    })
                    .unwrap(),
            );
        }
        json!({
            "shared_library": self.hito.catalog.libraries(&self.actor).await.unwrap().into_iter().find(|library| library.id == self.shared).unwrap(),
            "recordings": self.hito.catalog.recordings(&self.actor, Default::default()).await.unwrap(),
            "credit_pages": credit_pages,
            "releases": self.hito.catalog.releases(&self.actor, Default::default()).await.unwrap(),
            "metadata": metadata, "sources": sources, "receipt": self.receipt,
            "playlist": self.hito.collection.playlist(&self.actor, &self.receipt.playlist_id.to_string()).await.unwrap(),
            "activity": self.hito.collection.activity(&self.actor).await.unwrap(),
            "progress": self.hito.collection.saved_playback(&self.actor, &[
                Uuid::parse_str(&self.local).unwrap(), Uuid::parse_str(&self.imported).unwrap(), Uuid::parse_str(&self.namesake).unwrap(),
            ]).await.unwrap(),
            "events": self.hito.collection.events(&self.actor, 0).await.unwrap(),
            "settings": self.hito.catalog.metadata_settings(&self.actor).await.unwrap(),
            "plugins": self.hito.plugins.settings(&self.actor).await.unwrap(),
            "peer_recordings": self.hito.catalog.recordings(&self.peer, Default::default()).await.unwrap(),
            "peer_artists": self.hito.catalog.artists(&self.peer, Default::default()).await.unwrap(),
            "peer_releases": self.hito.catalog.releases(&self.peer, Default::default()).await.unwrap(),
        })
    }

    pub async fn origin_requests(&self) -> Vec<usize> {
        let mut counts = Vec::new();
        for path in [
            "v1/manifest",
            "v1/playlists/detail",
            "v1/playlists/items",
            "v1/recordings/detail",
            "v1/recordings/metadata",
            "v1/sources",
            "v1/audio",
        ] {
            counts.push(self.origin.requests(path).await.len());
        }
        counts
    }
}
