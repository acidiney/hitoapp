use hito_core::test_support::artist_identity_selection_catalog as catalog;

use crate::{
    artist_review_fixture::{self as review, Account, Imported},
    provider_fixture::ProviderServer,
    support,
};
use axum::{Router, http::StatusCode};
use hito_core::{ArtistReviewContext, ChooseRecordingArtistIdentity, MetadataProvider};
use hito_plugin_sdk::{ArtistProfileRequest, CatalogArtistProfile};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;

pub(super) const ARTIST_ID: &str = catalog::ALPHA;

pub(super) struct Library {
    pub app: Router,
    pub owner: Account,
    pub local: String,
    pub imported: String,
    pub other: String,
    pub provider: MetadataProvider,
    pub peer: Account,
    pub credit_ids: Vec<String>,
    playlist: String,
    origin: ProviderServer,
    catalog: catalog::CatalogServer,
    pub audio: PathBuf,
    original: Vec<u8>,
    _directory: tempfile::TempDir,
}

impl Library {
    pub async fn prepare() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let audio = music.join("owned-identity.wav");
        support::write_wav(&audio);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Owned local HTTP recording".into());
        tag.set_artist(review::CREDIT.into());
        tag.set_album("Owned local HTTP album".into());
        tag.insert_text(ItemKey::AlbumArtist, "Distinct local album credit".into());
        tag.save_to_path(&audio, Default::default()).unwrap();
        let original = std::fs::read(&audio).unwrap();
        let services = support::services(directory.path(), &music).await;
        let app = hito_server::router(services.clone());
        let owner = Account::setup(&app).await;
        let origin = ProviderServer::start().await;
        let imported = Imported::create(&app, &owner, &origin).await;
        let libraries = owner.get(&app, "/api/v1/libraries").await;
        let shared = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|library| library["personal"] == false)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (status, _) = owner
            .request(
                &app,
                "POST",
                &format!("/api/v1/libraries/{shared}/scan"),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        services.storage.run_next_job().await.unwrap();
        let local_page = owner
            .get(&app, &format!("/api/v1/recordings?library={shared}"))
            .await;
        assert_eq!(local_page["total"], 1);
        let local = local_page["items"][0]["id"].as_str().unwrap().to_owned();
        assert!(local != imported.recording && local != imported.other_recording);
        let playlists = owner.get(&app, "/api/v1/playlists").await;
        assert_eq!(playlists.as_array().unwrap().len(), 1);
        let playlist = playlists[0]["id"].as_str().unwrap().to_owned();
        let copy = owner
            .get(&app, &format!("/api/v1/playlists/{playlist}"))
            .await;
        assert_eq!(copy["entries"].as_array().unwrap().len(), 3);
        assert_eq!(copy["entries"][0]["recording_id"], imported.recording);
        assert_eq!(copy["entries"][1]["recording_id"], imported.other_recording);
        assert_eq!(copy["entries"][2]["recording_id"], imported.recording);
        assert_ne!(
            copy["entries"][0]["entry_id"],
            copy["entries"][2]["entry_id"]
        );
        let (status, user) = owner
            .request(
                &app,
                "POST",
                "/api/v1/users",
                json!({"username": "identity-peer", "password": "owned HTTP identity peer password"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = Account::login(&app, "identity-peer", "owned HTTP identity peer password").await;
        assert_eq!(user["id"], peer.id);
        let (status, _) = owner
            .request(
                &app,
                "PUT",
                &format!("/api/v1/libraries/{shared}/members/{}", peer.id),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let catalog = catalog::CatalogServer::start().await;
        let installed = owner
            .connect(&app, &catalog.url, "Owned HTTP identity catalog", None)
            .await;
        let provider = MetadataProvider::HttpPlugin {
            installation_id: installed["id"].as_str().unwrap().into(),
        };
        let settings = owner.get(&app, "/api/v1/metadata/settings").await;
        let (status, _) = owner
            .request(
                &app,
                "PUT",
                "/api/v1/metadata/settings",
                json!({"revision": settings["revision"], "enabled": true, "providers": [provider]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let credit_ids = owner.get(&app, "/api/v1/artists").await["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|artist| artist["id"].as_str().unwrap().to_owned())
            .collect();
        Self {
            app,
            owner,
            local,
            imported: imported.recording,
            other: imported.other_recording,
            provider,
            peer,
            credit_ids,
            playlist,
            origin,
            catalog,
            audio,
            original,
            _directory: directory,
        }
    }

    pub async fn review(&self, recording: &str) -> (ArtistReviewContext, String) {
        let path = format!("/api/v1/recordings/{recording}/artist-review");
        let (status, value) = self
            .owner
            .request(
                &self.app,
                "POST",
                &format!("{path}/context"),
                json!({"provider": self.provider}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let context: ArtistReviewContext = serde_json::from_value(value).unwrap();
        assert_eq!(context.recording_id, recording);
        assert_eq!(context.account_id, self.owner.id);
        assert_eq!(context.provider, self.provider);
        let (status, page) = self
            .owner
            .request(
                &self.app,
                "POST",
                &format!("{path}/search"),
                json!({"context": context, "search": {"query": catalog::CREDIT, "limit": 2}}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["page"]["items"].as_array().unwrap().len(), 2);
        assert_ne!(
            page["page"]["items"][0]["artist"]["id"],
            page["page"]["items"][1]["artist"]["id"]
        );
        assert_eq!(
            page["page"]["items"][0]["artist"]["name"],
            page["page"]["items"][1]["artist"]["name"]
        );
        let selection = json!({"context": context, "provider_artist_id": ARTIST_ID});
        let (status, preview) = self
            .owner
            .request(
                &self.app,
                "POST",
                &format!("{path}/preview"),
                selection.clone(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let profile: CatalogArtistProfile =
            serde_json::from_value(preview["profile"].clone()).unwrap();
        profile
            .validate_for(&ArtistProfileRequest {
                artist_id: ARTIST_ID.into(),
            })
            .unwrap();
        assert_eq!(profile.artist.id, ARTIST_ID);
        assert_eq!(profile.artist.name, catalog::CREDIT);
        assert_eq!(profile.revision.as_deref(), Some("owned-profile-v1"));
        let (status, association) = self
            .owner
            .request(&self.app, "POST", &format!("{path}/confirm"), selection)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(association["recording_id"], recording);
        assert_eq!(association["provider"], json!(self.provider));
        assert_eq!(association["provider_artist_id"], ARTIST_ID);
        (context, association["id"].as_str().unwrap().into())
    }

    pub async fn provider_counts(&self) -> (usize, Vec<usize>) {
        (
            self.catalog.requests().len(),
            review::origin_counts(&self.origin).await,
        )
    }

    pub async fn select(&self, recording: &str) -> (Value, Value) {
        let (context, association) = self.review(recording).await;
        let request = json!(ChooseRecordingArtistIdentity {
            request_id: Uuid::new_v4(),
            expected_revision: 0,
            association_id: Uuid::parse_str(&association).unwrap(),
            context,
        });
        let (status, receipt) = self
            .owner
            .request(
                &self.app,
                "PUT",
                &format!("/api/v1/recordings/{recording}/artist-identity"),
                request.clone(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        (request, receipt)
    }

    pub async fn snapshot(&self) -> Value {
        assert_eq!(std::fs::read(&self.audio).unwrap(), self.original);
        let mut metadata = Vec::new();
        let mut sources = Vec::new();
        let mut associations = Vec::new();
        for recording in [&self.local, &self.imported, &self.other] {
            metadata.push(
                self.owner
                    .get(
                        &self.app,
                        &format!("/api/v1/recordings/{recording}/metadata"),
                    )
                    .await,
            );
            sources.push(
                self.owner
                    .get(
                        &self.app,
                        &format!("/api/v1/recordings/{recording}/sources"),
                    )
                    .await,
            );
            associations.push(
                self.owner
                    .get(
                        &self.app,
                        &format!("/api/v1/recordings/{recording}/artist-associations"),
                    )
                    .await,
            );
            let (status, _) = self
                .owner
                .request(
                    &self.app,
                    "GET",
                    &format!("/api/v1/recordings/{recording}/download"),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
        assert_eq!(sources[0].as_array().unwrap().len(), 1);
        assert_eq!(sources[1], json!([]));
        assert_eq!(sources[2], json!([]));
        let (status, progress) = self
            .owner
            .request(
                &self.app,
                "POST",
                "/api/v1/recordings/progress",
                json!({"recording_ids": [&self.local, &self.imported, &self.other]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let mut credit_pages = Vec::new();
        for id in &self.credit_ids {
            credit_pages.push(
                self.owner
                    .get(&self.app, &format!("/api/v1/artists/{id}"))
                    .await,
            );
        }
        json!({
            "recordings": self.owner.get(&self.app, "/api/v1/recordings").await,
            "credit_pages": credit_pages,
            "releases": self.owner.get(&self.app, "/api/v1/releases").await,
            "libraries": self.owner.get(&self.app, "/api/v1/libraries").await,
            "metadata": metadata, "sources": sources, "associations": associations, "progress": progress,
            "playlists": self.owner.get(&self.app, "/api/v1/playlists").await,
            "copy": self.owner.get(&self.app, &format!("/api/v1/playlists/{}", self.playlist)).await,
            "activity": self.owner.get(&self.app, "/api/v1/activity").await,
            "events": self.owner.get(&self.app, "/api/v1/sync?cursor=0").await,
            "settings": self.owner.get(&self.app, "/api/v1/metadata/settings").await,
            "plugins": self.owner.get(&self.app, "/api/v1/plugins").await,
            "peer_recordings": self.peer.get(&self.app, "/api/v1/recordings").await,
            "peer_artists": self.peer.get(&self.app, "/api/v1/artists").await,
            "peer_releases": self.peer.get(&self.app, "/api/v1/releases").await,
            "peer_playlists": self.peer.get(&self.app, "/api/v1/playlists").await,
            "peer_activity": self.peer.get(&self.app, "/api/v1/activity").await,
            "peer_events": self.peer.get(&self.app, "/api/v1/sync?cursor=0").await,
        })
    }

    pub async fn finish(mut self) {
        self.origin.shutdown().await;
        self.catalog.finish().await;
    }
}
