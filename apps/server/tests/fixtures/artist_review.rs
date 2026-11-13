pub(super) use crate::account::{Account, exchange};
pub(super) use hito_core::test_support::artist_catalog_discovery_service as catalog;

use crate::provider_fixture::ProviderServer;
use axum::{Router, http::StatusCode};
use hito_core::{Action, Operation};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION, PlaylistAttribution, PlaylistPage, ProviderPlaylist,
    ProviderPlaylistEntry, ProviderPlaylistItem, ProviderRecording, ProviderRelease,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) const CREDIT: &str = "Owned HTTP imported performer";
const ORIGIN: &str = "org.hito.http-artist-review-playlist";
const FIRST: &str = "spotify:track:owned-http-one";
const SECOND: &str = "spotify:track:owned-http-two";

pub(super) struct Imported {
    pub recording: String,
    pub other_recording: String,
    pub artist: String,
    pub observation: String,
    playlist: String,
}

impl Imported {
    pub async fn create(app: &Router, account: &Account, origin: &ProviderServer) -> Self {
        origin
            .reply(
                "v1/manifest",
                json!(Manifest {
                    id: ORIGIN.into(),
                    name: "Owned HTTP playlist metadata".into(),
                    protocol_version: PROTOCOL_VERSION,
                    capabilities: [Capability::Playlists].into(),
                    authorization: None,
                    content_policy: None,
                }),
            )
            .await;
        let detail = ProviderPlaylist {
            id: "spotify:playlist:owned-http-artist-review".into(),
            name: "Private HTTP artist review selection".into(),
            snapshot: "owned-http-artist-review:1".into(),
            total_count: 3,
            attribution: PlaylistAttribution {
                label: "Owned HTTP playlist fixture".into(),
            },
        };
        let page = PlaylistPage {
            playlist_id: detail.id.clone(),
            snapshot: detail.snapshot.clone(),
            items: [FIRST, SECOND, FIRST]
                .into_iter()
                .enumerate()
                .map(|(index, id)| ProviderPlaylistEntry {
                    occurrence_id: Some(format!("owned-http-occurrence-{index}")),
                    item: ProviderPlaylistItem::Recording {
                        recording: Box::new(recording(id)),
                    },
                })
                .collect(),
            next_cursor: None,
        };
        assert!(detail.validate().is_ok() && page.validate(100).is_ok());
        origin.reply("v1/playlists/detail", json!(detail)).await;
        origin.reply("v1/playlists/items", json!(page)).await;
        let installation = account
            .connect(app, &origin.url, "Owned HTTP playlist", None)
            .await;
        let import_path = format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        );
        let (status, receipt) = account
            .request(
                app,
                "POST",
                &import_path,
                json!({
                    "request_id": Uuid::new_v4(),
                    "installation_revision": installation["revision"],
                    "playlist_reference": "https://provider.invalid/owned-http-artist-review",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
        let copy = account
            .get(app, &format!("/api/v1/playlists/{playlist}"))
            .await;
        let entries = copy["entries"].as_array().unwrap();
        assert!(entries.len() == 3 && copy["playlist"]["kind"] == "local");
        let recording = entries[0]["recording_id"].as_str().unwrap().to_owned();
        let other_recording = entries[1]["recording_id"].as_str().unwrap().to_owned();
        assert!(recording != other_recording && entries[2]["recording_id"] == recording);
        assert!(entries[0]["entry_id"] != entries[2]["entry_id"]);
        assert!(
            entries
                .iter()
                .enumerate()
                .all(|(index, entry)| entry["position"] == index && entry["available"] == false)
        );
        let recorded = Uuid::parse_str(&recording).unwrap();
        let operations = [
            Action::RecordingTitleSet {
                recording_id: recorded,
                title: "Private HTTP title pin".into(),
            },
            Action::RecordingArtistSet {
                recording_id: recorded,
                artist: CREDIT.into(),
            },
            Action::LikeSet {
                recording_id: recorded,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recorded,
                position_ms: 400,
                speed: 1.25,
            },
        ]
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect::<Vec<_>>();
        let (status, accepted) = account
            .request(
                app,
                "POST",
                "/api/v1/sync",
                json!({
                    "device_id": "owned-http-artist-review", "operations": operations,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            accepted["acknowledged"]
                == json!(
                    operations
                        .iter()
                        .map(|operation| operation.id)
                        .collect::<Vec<_>>()
                )
        );
        let metadata = account
            .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
            .await;
        assert!(metadata["title"]["personal"] == "Private HTTP title pin");
        assert!(metadata["artist"]["personal"] == CREDIT);
        let observation = &metadata["selected_observation"];
        assert!(observation["source_id"].is_null() && observation["origin"] == "provider_response");
        assert!(
            observation["provider"] == ORIGIN
                && observation["plugin_installation_id"] == installation["id"]
        );
        let artists = account.get(app, "/api/v1/artists").await;
        assert!(artists["total"] == 1 && artists["items"][0]["recording_count"] == 2);
        Self {
            recording,
            other_recording,
            playlist,
            artist: artists["items"][0]["id"].as_str().unwrap().into(),
            observation: observation["id"].as_str().unwrap().into(),
        }
    }

    pub async fn snapshot(&self, app: &Router, account: &Account) -> Value {
        let mut metadata = Vec::new();
        let mut sources = Vec::new();
        let mut downloads = Vec::new();
        for recording in [&self.recording, &self.other_recording] {
            metadata.push(
                account
                    .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
                    .await,
            );
            let current = account
                .get(app, &format!("/api/v1/recordings/{recording}/sources"))
                .await;
            assert!(current == json!([]));
            sources.push(current);
            let (status, absent) = account
                .request(
                    app,
                    "GET",
                    &format!("/api/v1/recordings/{recording}/download"),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            downloads.push(json!({"status": status.as_u16(), "body": absent}));
        }
        let (status, progress) = account
            .request(
                app,
                "POST",
                "/api/v1/recordings/progress",
                json!({
                    "recording_ids": [&self.recording, &self.other_recording],
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let catalog = account.get(app, "/api/v1/recordings").await;
        assert!(
            catalog["total"] == 2
                && catalog["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|recording| recording["available"] == false)
        );
        json!({
            "catalog": catalog, "metadata": metadata, "sources": sources, "downloads": downloads,
            "artists": account.get(app, "/api/v1/artists").await,
            "releases": account.get(app, "/api/v1/releases").await,
            "libraries": account.get(app, "/api/v1/libraries").await,
            "playlist": account.get(app, &format!("/api/v1/playlists/{}", self.playlist)).await,
            "activity": account.get(app, "/api/v1/activity").await,
            "events": account.get(app, "/api/v1/sync?cursor=0").await, "progress": progress,
            "plugins": account.get(app, "/api/v1/plugins").await,
            "policy": account.get(app, "/api/v1/metadata/settings").await,
        })
    }
}

pub(super) async fn origin_counts(origin: &ProviderServer) -> Vec<usize> {
    let mut counts = Vec::new();
    for path in [
        "v1/manifest",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/audio",
    ] {
        counts.push(origin.requests(path).await.len());
    }
    counts
}

fn recording(id: &str) -> ProviderRecording {
    ProviderRecording {
        isrc: None,
        artist_urls: Vec::new(),
        artists: Vec::new(),
        id: id.into(),
        title: if id == FIRST {
            "Owned first HTTP recording"
        } else {
            "Owned second HTTP recording"
        }
        .into(),
        artist: CREDIT.into(),
        duration_ms: Some(1000),
        release: Some(ProviderRelease {
            id: "spotify:album:owned-http".into(),
            title: "Owned HTTP album".into(),
            artist: Some(CREDIT.into()),
        }),
        disc_number: Some(1),
        track_number: Some(if id == FIRST { 1 } else { 2 }),
        revision: Some("owned-http-recording:1".into()),
        attribution: None,
    }
}
