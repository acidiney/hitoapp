use super::{http, provider, support};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    Action, Actor, AdoptProviderRecording, FollowProviderPlaylist, ImportProviderPlaylist,
    PlaylistFollowInterval, Services,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub const RECORDING: &str = "recording/stable-availability";
pub const PLAYLIST: &str = "playlist/stable-availability";
pub const OCCURRENCE: &str = "stable-occurrence";
pub const SOURCE: &str = "audio/stable-availability";

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub provider: provider::ProviderServer,
    pub app: Router,
    pub token: String,
    pub recording: String,
    pub playlist: String,
    pub copy: Uuid,
    pub original: Vec<u8>,
    pub audio: Vec<u8>,
}

impl Fixture {
    pub async fn start() -> Self {
        let (directory, services, actor, library, music) =
            support::library("follow-download-availability").await;
        let original = support::write_wav(&music.join("original.wav"), 29);
        support::scan(&services, &actor, &library).await;
        let audio = support::write_wav(&directory.path().join("provider.wav"), 73);
        assert_ne!(original, audio);
        let provider = provider::ProviderServer::start().await;
        let installation =
            provider::connect(&services, &actor, &provider, &["sources", "playlists"]).await;
        provider.reply("v1/recordings/detail", recording()).await;
        provider
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": SOURCE, "label": "Owned original", "ready": true
                }]}),
            )
            .await;
        provider
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([
                    (
                        "content-type".parse().unwrap(),
                        "audio/wav".parse().unwrap(),
                    ),
                    (
                        "content-length".parse().unwrap(),
                        audio.len().to_string().parse().unwrap(),
                    ),
                ]),
                audio.clone(),
            )
            .await;
        let adopted = services
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: installation.revision,
                    provider_recording_id: RECORDING.into(),
                },
            )
            .await
            .unwrap();
        let recording = adopted.id;
        let sources = services.catalog.sources(&actor, &recording).await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].provider, "plugin");
        assert!(sources[0].ready);
        snapshot(&provider, "independent-copy", true).await;
        let copied = services
            .collection
            .import_provider_playlist(
                &actor,
                &installation.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installation.revision,
                    playlist_reference: "https://provider.invalid/stable-availability".into(),
                },
            )
            .await
            .unwrap();
        let recording_id = Uuid::parse_str(&recording).unwrap();
        support::synchronize(
            &services,
            &actor,
            vec![
                Action::PlaylistRename {
                    playlist_id: copied.playlist_id,
                    name: "My independent copy".into(),
                },
                Action::RecordingTitleSet {
                    recording_id,
                    title: "My private title".into(),
                },
                Action::RecordingArtistSet {
                    recording_id,
                    artist: "My private artist".into(),
                },
                Action::LikeSet {
                    recording_id,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id,
                    position_ms: 700,
                    speed: 1.25,
                },
            ],
        )
        .await;
        snapshot(&provider, "initial-unavailable", false).await;
        let followed = services
            .collection
            .follow_provider_playlist(
                &actor,
                &installation.id,
                FollowProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installation.revision,
                    playlist_reference: "https://provider.invalid/stable-availability".into(),
                    interval: PlaylistFollowInterval::Hourly,
                },
            )
            .await
            .unwrap();
        let app = hito_server::router(services.clone());
        let (status, session) = http::call(
            &app,
            "POST",
            "/api/v1/login",
            json!({
                "username": "admin", "password": "indexing test password"
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let token = session["token"].as_str().unwrap().to_owned();
        Self {
            directory,
            services,
            actor,
            provider,
            app,
            token,
            recording,
            playlist: followed.playlist_id.to_string(),
            copy: copied.playlist_id,
            original,
            audio,
        }
    }

    pub async fn saved_state(&self) -> Value {
        let mut catalog = self.get("/api/v1/recordings?limit=100").await;
        let mut copy = self.get(&format!("/api/v1/playlists/{}", self.copy)).await;
        let mut activity = self.get("/api/v1/activity").await;
        for rows in [
            &mut catalog["items"],
            &mut copy["entries"],
            &mut activity["history"],
        ] {
            for row in rows.as_array_mut().unwrap() {
                row.as_object_mut().unwrap().remove("available");
            }
        }
        let metadata = self
            .get(&format!("/api/v1/recordings/{}/metadata", self.recording))
            .await;
        json!({"catalog": catalog, "copy": copy, "activity": activity, "metadata": {
            "recording_id": metadata["recording_id"], "title": metadata["title"], "artist": metadata["artist"]
        }})
    }

    pub async fn get(&self, path: &str) -> Value {
        let (status, value) =
            http::call(&self.app, "GET", path, Value::Null, Some(&self.token)).await;
        assert_eq!(status, StatusCode::OK);
        value
    }
}

pub async fn snapshot(provider: &provider::ProviderServer, revision: &str, available: bool) {
    let item = if available {
        json!({"kind": "recording", "recording": recording()})
    } else {
        json!({"kind": "unavailable", "provider_recording_id": RECORDING,
            "title": "Temporarily unavailable", "artist": "Owned artist"})
    };
    provider::playlist(
        provider,
        PLAYLIST,
        revision,
        vec![json!({
            "occurrence_id": OCCURRENCE, "item": item
        })],
    )
    .await;
}

fn recording() -> Value {
    json!({"id": RECORDING, "title": "Owned available recording", "artist": "Owned artist", "duration_ms": 1000})
}
