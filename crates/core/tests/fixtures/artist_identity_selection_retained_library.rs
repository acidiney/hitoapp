use crate::{catalog, support};
use hito_core::{
    Action, Actor, Credentials, InstallPlugin, MetadataProvider, OfferPluginService, Services,
    UpdateMetadataSettings,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;

pub struct RetainedLibrary {
    pub hito: Services,
    pub admin: Actor,
    pub listener: Actor,
    pub library_id: String,
    pub recording_id: String,
    pub provider: MetadataProvider,
    pub catalog: catalog::CatalogServer,
    audio: PathBuf,
    original: Vec<u8>,
    _directory: tempfile::TempDir,
}

impl RetainedLibrary {
    pub async fn prepare() -> Self {
        let (directory, hito, admin, library_id, music) =
            support::library("retained-artist-identity").await;
        let audio = music.join("owned.wav");
        support::write_wav(&audio, 0);
        tag(catalog::CREDIT)
            .save_to_path(&audio, Default::default())
            .unwrap();
        let original = std::fs::read(&audio).unwrap();
        support::scan(&hito, &admin, &library_id).await;
        let recording_id = hito
            .catalog
            .recordings(&admin, Default::default())
            .await
            .unwrap()
            .items
            .remove(0)
            .id;
        let user = hito
            .identity
            .create_user(
                &admin,
                Credentials {
                    username: "listener".into(),
                    password: "owned retained identity password".into(),
                },
            )
            .await
            .unwrap();
        hito.identity
            .set_library_access(&admin, &library_id, &user.id, true)
            .await
            .unwrap();
        let session = hito
            .identity
            .login(Credentials {
                username: "listener".into(),
                password: "owned retained identity password".into(),
            })
            .await
            .unwrap();
        let listener = hito.identity.authenticate(&session.token).await.unwrap();
        let id = Uuid::parse_str(&recording_id).unwrap();
        support::synchronize(
            &hito,
            &listener,
            vec![
                Action::RecordingTitleSet {
                    recording_id: id,
                    title: "Private retained title".into(),
                },
                Action::RecordingArtistSet {
                    recording_id: id,
                    artist: catalog::CREDIT.into(),
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
            ],
        )
        .await;
        let catalog = catalog::CatalogServer::start().await;
        let service = hito
            .plugins
            .offer_service(
                &admin,
                OfferPluginService {
                    name: "Owned retained artist catalog".into(),
                    base_url: catalog.url.clone(),
                },
            )
            .await
            .unwrap();
        let installed = hito
            .plugins
            .install(
                &listener,
                InstallPlugin {
                    service_id: service.id,
                    service_revision: service.revision,
                    accept_manifest_digest: None,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let provider = MetadataProvider::HttpPlugin {
            installation_id: installed.id,
        };
        let settings = hito.catalog.metadata_settings(&listener).await.unwrap();
        hito.catalog
            .set_metadata_settings(
                &listener,
                UpdateMetadataSettings {
                    revision: settings.revision,
                    enabled: true,
                    providers: vec![provider.clone()],
                },
            )
            .await
            .unwrap();
        Self {
            hito,
            admin,
            listener,
            library_id,
            recording_id,
            provider,
            catalog,
            audio,
            original,
            _directory: directory,
        }
    }

    pub async fn access(&self, allowed: bool) {
        self.hito
            .identity
            .set_library_access(
                &self.admin,
                &self.library_id,
                &self.listener.user().id,
                allowed,
            )
            .await
            .unwrap();
    }

    pub async fn retag(&self) {
        tag("Intervening source performer")
            .save_to_path(&self.audio, Default::default())
            .unwrap();
        support::scan(&self.hito, &self.admin, &self.library_id).await;
    }

    pub async fn restore(&self) {
        std::fs::write(&self.audio, &self.original).unwrap();
        support::scan(&self.hito, &self.admin, &self.library_id).await;
    }

    pub async fn listed_credits(&self) -> Vec<String> {
        self.hito
            .catalog
            .artists(&self.listener, Default::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|artist| artist.name)
            .collect()
    }

    pub async fn snapshot(&self) -> Value {
        assert_eq!(std::fs::read(&self.audio).unwrap(), self.original);
        let metadata = self
            .hito
            .catalog
            .recording_metadata(&self.listener, &self.recording_id, Default::default())
            .await
            .unwrap();
        json!({
            "recordings": self.hito.catalog.recordings(&self.listener, Default::default()).await.unwrap(),
            "releases": self.hito.catalog.releases(&self.listener, Default::default()).await.unwrap(),
            "title": metadata.title, "artist": metadata.artist, "selected_observation": metadata.selected_observation,
            "sources": self.hito.catalog.sources(&self.listener, &self.recording_id).await.unwrap(),
            "activity": self.hito.collection.activity(&self.listener).await.unwrap(),
            "progress": self.hito.collection.saved_playback(&self.listener, &[Uuid::parse_str(&self.recording_id).unwrap()]).await.unwrap(),
            "events": self.hito.collection.events(&self.listener, 0).await.unwrap(),
            "associations": self.hito.catalog.recording_artist_associations(&self.listener, &self.recording_id).await.unwrap(),
            "settings": self.hito.catalog.metadata_settings(&self.listener).await.unwrap(),
            "plugins": self.hito.plugins.settings(&self.listener).await.unwrap(),
            "admin_recordings": self.hito.catalog.recordings(&self.admin, Default::default()).await.unwrap(),
            "admin_artists": self.hito.catalog.artists(&self.admin, Default::default()).await.unwrap(),
        })
    }
}

fn tag(artist: &str) -> Tag {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Owned retained recording".into());
    tag.set_artist(artist.into());
    tag.set_album("Owned retained album".into());
    tag.insert_text(ItemKey::AlbumArtist, "Retained album credit".into());
    tag
}
