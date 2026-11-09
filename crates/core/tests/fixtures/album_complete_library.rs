use super::provider::{self, AlbumProvider};
use super::support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, AlbumEditionTrackListAvailability, ArtistAssociation,
    ArtistCreditRole, AssociateRecordingArtist, Browse, InstallPlugin, MetadataProvider,
    OfferPluginService, PluginInstallation, ReadAlbumEditionTrackList, Recording, Search, Services,
    UpdateMetadataSettings,
};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, sqlite::SqliteConnectOptions};
use std::path::PathBuf;
use uuid::Uuid;

pub const PERSONAL_TITLE: &str = "Private personal title";

pub struct Album {
    pub hito: Services,
    pub actor: Actor,
    pub installation: PluginInstallation,
    pub association: ArtistAssociation,
    pub release_id: String,
    pub first: String,
    pub second: String,
    pub extra: String,
    pub featured: Option<String>,
    database: PathBuf,
    _directory: tempfile::TempDir,
}

pub async fn connect(hito: &Services, actor: &Actor, server: &AlbumProvider) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned album editions".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

pub async fn adopt(
    hito: &Services,
    actor: &Actor,
    installation: &PluginInstallation,
    id: &str,
) -> Recording {
    hito.catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: id.into(),
            },
        )
        .await
        .unwrap()
}

impl Album {
    pub async fn prepare(server: &AlbumProvider) -> Self {
        Self::build(server, false).await
    }

    pub async fn prepare_with_featured(server: &AlbumProvider) -> Self {
        Self::build(server, true).await
    }

    async fn build(server: &AlbumProvider, featured: bool) -> Self {
        let (directory, hito, actor, _library, _music) = support::library("album-complete").await;
        let installation = connect(&hito, &actor, server).await;
        let mut saved = Vec::new();
        for id in [provider::FIRST, provider::SECOND, provider::EXTRA] {
            saved.push(adopt(&hito, &actor, &installation, id).await);
        }
        let featured_recording = if featured {
            Some(adopt(&hito, &actor, &installation, provider::FEATURED).await)
        } else {
            None
        };
        let release_id = saved[0].release_id.clone().expect("a provider release");
        assert!(
            saved
                .iter()
                .all(|recording| recording.release_id.as_ref() == Some(&release_id))
        );
        support::synchronize(
            &hito,
            &actor,
            vec![Action::RecordingTitleSet {
                recording_id: Uuid::parse_str(&saved[1].id).unwrap(),
                title: PERSONAL_TITLE.into(),
            }],
        )
        .await;
        let settings = hito.artist_catalog.metadata_settings(&actor).await.unwrap();
        let selected = MetadataProvider::HttpPlugin {
            installation_id: installation.id.clone(),
        };
        let settings = hito
            .artist_catalog
            .set_metadata_settings(
                &actor,
                UpdateMetadataSettings {
                    revision: settings.revision,
                    enabled: true,
                    providers: vec![selected.clone()],
                },
            )
            .await
            .unwrap();
        let artists = hito
            .catalog
            .artists(&actor, Browse::default())
            .await
            .unwrap();
        assert_eq!(artists.total, if featured { 2 } else { 1 });
        let associate = |recording: String, credit: &'static str| {
            let hito = &hito;
            let actor = &actor;
            let selected = selected.clone();
            let artist = artists
                .items
                .iter()
                .find(|artist| artist.name == credit)
                .expect("the credited artist")
                .id
                .clone();
            async move {
                let metadata = hito
                    .catalog
                    .recording_metadata(actor, &recording, Default::default())
                    .await
                    .unwrap();
                hito.artist_catalog
                    .associate_recording_artist(
                        actor,
                        &recording,
                        AssociateRecordingArtist {
                            artist_id: artist,
                            source_id: None,
                            observation_id: metadata.selected_observation.unwrap().id,
                            role: ArtistCreditRole::RecordingArtist,
                            provider: selected,
                            provider_artist_id: provider::ARTIST_ID.into(),
                            policy_revision: settings.revision,
                        },
                    )
                    .await
                    .unwrap()
            }
        };
        if let Some(recording) = &featured_recording {
            associate(recording.id.clone(), provider::FEATURED_CREDIT).await;
        }
        let association = associate(saved[0].id.clone(), provider::CREDIT).await;
        Self {
            database: directory.path().join("hito.db"),
            hito,
            actor,
            installation,
            association,
            release_id,
            first: saved[0].id.clone(),
            second: saved[1].id.clone(),
            extra: saved[2].id.clone(),
            featured: featured_recording.map(|recording| recording.id),
            _directory: directory,
        }
    }

    pub fn request(&self, edition: &str) -> ReadAlbumEditionTrackList {
        ReadAlbumEditionTrackList {
            recording_id: self.first.clone(),
            association_id: self.association.id.clone(),
            release_group_id: provider::GROUP_ID.into(),
            edition_id: edition.into(),
        }
    }

    pub async fn read(
        &self,
        edition: &str,
    ) -> hito_core::Result<AlbumEditionTrackListAvailability> {
        self.hito
            .artist_catalog
            .album_edition_track_list_until(
                &self.actor,
                &self.release_id,
                self.request(edition),
                std::future::pending(),
            )
            .await
    }

    pub async fn library(&self) -> Value {
        let mut release = serde_json::to_value(
            self.hito
                .catalog
                .release(&self.actor, &self.release_id, Browse::default())
                .await
                .unwrap(),
        )
        .unwrap();
        release.as_object_mut().unwrap().remove("catalog_anchor");
        json!({
            "recordings": self.hito.catalog.recordings(&self.actor, Search::default()).await.unwrap(),
            "release": release,
        })
    }

    pub async fn total(&self) -> i64 {
        self.hito
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .total
    }

    pub async fn sql(&self, statement: &str, binds: &[&str]) {
        let mut connection = SqliteConnectOptions::new()
            .filename(&self.database)
            .foreign_keys(false)
            .connect()
            .await
            .unwrap();
        let mut query = sqlx::query(statement);
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(&mut connection).await.unwrap();
    }

    pub async fn count(&self, statement: &str, binds: &[&str]) -> i64 {
        let mut connection = SqliteConnectOptions::new()
            .filename(&self.database)
            .foreign_keys(false)
            .connect()
            .await
            .unwrap();
        let mut query = sqlx::query_scalar(statement);
        for bind in binds {
            query = query.bind(*bind);
        }
        query.fetch_one(&mut connection).await.unwrap()
    }
}
