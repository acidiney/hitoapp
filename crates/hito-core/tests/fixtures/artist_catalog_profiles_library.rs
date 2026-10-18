use crate::support;
use hito_core::{
    Action, Actor, ArtistAssociation, ArtistCreditRole, AssociateRecordingArtist, Browse,
    Credentials, InstallPlugin, MetadataHistoryQuery, MetadataProvider, OfferPluginService, Search,
    Services, UpdateMetadataSettings,
};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::Value;
use uuid::Uuid;

pub const CREDIT: &str = "Shared exact performer credit";

pub struct Library {
    pub hito: Services,
    pub admin: Actor,
    pub actor: Actor,
    pub library_id: String,
    pub recording_id: String,
    pub other_recording_id: String,
    pub artist_id: String,
    pub observation_id: String,
    pub source_id: String,
    playlist_id: Uuid,
    _directory: tempfile::TempDir,
}

impl Library {
    pub async fn prepare() -> Self {
        let (directory, hito, admin, library_id, music) =
            support::library("artist-catalog-host").await;
        for (offset, title) in ["Reviewed recording", "Unreviewed namesake"]
            .into_iter()
            .enumerate()
        {
            let path = music.join(format!("recording-{offset}.wav"));
            support::write_wav(&path, offset as i16);
            let mut tags = Tag::new(TagType::Id3v2);
            tags.set_title(title.into());
            tags.set_artist(CREDIT.into());
            tags.set_album("Owned album".into());
            tags.insert_text(ItemKey::AlbumArtist, CREDIT.into());
            tags.save_to_path(path, WriteOptions::default()).unwrap();
        }
        support::scan(&hito, &admin, &library_id).await;
        let user = hito
            .identity
            .create_user(
                &admin,
                Credentials {
                    username: "listener".into(),
                    password: "artist catalog listener password".into(),
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
                password: "artist catalog listener password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let recordings = hito
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(recordings.total, 2);
        let recording_id = recordings.items[0].id.clone();
        let other_recording_id = recordings.items[1].id.clone();
        assert_eq!(recordings.items[0].title, "Reviewed recording");
        assert_eq!(recordings.items[1].title, "Unreviewed namesake");
        assert_ne!(recording_id, other_recording_id);
        let playlist_id = Uuid::new_v4();
        let reviewed = Uuid::parse_str(&recording_id).unwrap();
        let namesake = Uuid::parse_str(&other_recording_id).unwrap();
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::PlaylistCreate {
                    playlist_id,
                    name: "Private artist review playlist".into(),
                },
                Action::EntryInsert {
                    playlist_id,
                    entry_id: Uuid::new_v4(),
                    recording_id: reviewed,
                    position: 0,
                },
                Action::EntryInsert {
                    playlist_id,
                    entry_id: Uuid::new_v4(),
                    recording_id: namesake,
                    position: 1,
                },
                Action::LikeSet {
                    recording_id: reviewed,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id: reviewed,
                    position_ms: 400,
                    speed: 1.25,
                },
                Action::RecordingTitleSet {
                    recording_id: reviewed,
                    title: "Private reviewed title".into(),
                },
                Action::RecordingArtistSet {
                    recording_id: reviewed,
                    artist: CREDIT.into(),
                },
            ],
        )
        .await;
        let mut artists = hito
            .catalog
            .artists(&actor, Browse::default())
            .await
            .unwrap();
        assert_eq!(artists.total, 1);
        let artist = artists.items.remove(0);
        assert_eq!(artist.name, CREDIT);
        assert_eq!(artist.recording_count, 2);
        let metadata = hito
            .catalog
            .recording_metadata(&actor, &recording_id, MetadataHistoryQuery::default())
            .await
            .unwrap();
        assert_eq!(
            metadata.title.personal.as_deref(),
            Some("Private reviewed title")
        );
        assert_eq!(metadata.artist.personal.as_deref(), Some(CREDIT));
        let observation = metadata.selected_observation.unwrap();
        assert_eq!(observation.artist, CREDIT);
        Self {
            hito,
            admin,
            actor,
            library_id,
            recording_id,
            other_recording_id,
            artist_id: artist.id,
            observation_id: observation.id,
            source_id: observation.source_id.unwrap(),
            playlist_id,
            _directory: directory,
        }
    }

    pub async fn review_artist(
        &self,
        provider_url: &str,
        provider_artist_id: &str,
    ) -> ArtistAssociation {
        let service = self
            .hito
            .plugins
            .offer_service(
                &self.admin,
                OfferPluginService {
                    name: "Owned artist catalog".into(),
                    base_url: provider_url.into(),
                },
            )
            .await
            .unwrap();
        let installation = self
            .hito
            .plugins
            .install(
                &self.actor,
                InstallPlugin {
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let settings = self
            .hito
            .catalog
            .metadata_settings(&self.actor)
            .await
            .unwrap();
        let provider = MetadataProvider::HttpPlugin {
            installation_id: installation.id,
        };
        let settings = self
            .hito
            .catalog
            .set_metadata_settings(
                &self.actor,
                UpdateMetadataSettings {
                    revision: settings.revision,
                    enabled: true,
                    providers: vec![provider.clone()],
                },
            )
            .await
            .unwrap();
        self.hito
            .catalog
            .associate_recording_artist(
                &self.actor,
                &self.recording_id,
                AssociateRecordingArtist {
                    artist_id: self.artist_id.clone(),
                    source_id: Some(self.source_id.clone()),
                    observation_id: self.observation_id.clone(),
                    role: ArtistCreditRole::RecordingArtist,
                    provider,
                    provider_artist_id: provider_artist_id.into(),
                    policy_revision: settings.revision,
                },
            )
            .await
            .unwrap()
    }

    pub async fn snapshot(&self) -> Value {
        let mut observations = Vec::new();
        let mut sources = Vec::new();
        for recording in [&self.recording_id, &self.other_recording_id] {
            observations.push(
                self.hito
                    .catalog
                    .recording_metadata(&self.actor, recording, MetadataHistoryQuery::default())
                    .await
                    .unwrap(),
            );
            sources.push(
                self.hito
                    .catalog
                    .sources(&self.actor, recording)
                    .await
                    .unwrap(),
            );
        }
        serde_json::json!({
            "recordings": self.hito.catalog.recordings(&self.actor, Search::default()).await.unwrap(),
            "artists": self.hito.catalog.artists(&self.actor, Browse::default()).await.unwrap(),
            "releases": self.hito.catalog.releases(&self.actor, Browse::default()).await.unwrap(),
            "observations": observations,
            "sources": sources,
            "playlist": self.hito.collection.playlist(&self.actor, &self.playlist_id.to_string()).await.unwrap(),
            "activity": self.hito.collection.activity(&self.actor).await.unwrap(),
            "progress": self.hito.collection.saved_playback(&self.actor, &[
                Uuid::parse_str(&self.recording_id).unwrap(),
                Uuid::parse_str(&self.other_recording_id).unwrap(),
            ]).await.unwrap(),
        })
    }
}
