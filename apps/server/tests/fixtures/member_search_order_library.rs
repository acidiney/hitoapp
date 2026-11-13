use crate::{
    account::Account, member_search_credential_order_fixture::accepted,
    provider_fixture::ProviderServer,
};
use axum::{Router, http::StatusCode};
use hito_core::{Action, Operation, test_support::playlist_service};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) const MEMBER: &str = "Owned ordering guest";

pub(crate) struct Imported {
    pub(crate) recording: String,
    pub(crate) member: String,
    playlist: String,
}

impl Imported {
    pub(crate) async fn create(app: &Router, owner: &Account, origin: &ProviderServer) -> Self {
        origin.reply("v1/manifest", json!({
            "id": "org.hito.member-order-playlist", "name": "Owned member ordering playlist",
            "protocol_version": 1, "capabilities": ["playlists"],
        })).await;
        playlist_service::playlist(origin, "owned-member-order", "owned-member-order:1", vec![json!({
            "occurrence_id": "owned-order-duet", "item": {"kind": "recording", "recording": {
                "id": "owned-member-order-duet", "title": "Owned ordering duet",
                "artist": "Owned ordering lead feat. Owned ordering guest", "duration_ms": 100_000,
                "artists": [
                    {"name": "Owned ordering lead", "role": "main", "join": " feat. "},
                    {"name": MEMBER, "role": "featured"}
                ]
            }}
        })]).await;
        let installation = owner
            .connect(app, &origin.url, "Owned member ordering metadata", None)
            .await;
        let receipt = accepted(
            app,
            owner,
            "POST",
            &format!(
                "/api/v1/plugins/{}/playlists/import",
                installation["id"].as_str().unwrap()
            ),
            json!({"request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
                "playlist_reference": "https://provider.invalid/owned-member-order"}),
        )
        .await;
        let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
        let copy = owner
            .get(app, &format!("/api/v1/playlists/{playlist}"))
            .await;
        assert_eq!(copy["entries"].as_array().unwrap().len(), 1);
        let recording = copy["entries"][0]["recording_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let recorded = Uuid::parse_str(&recording).unwrap();
        let operations = [
            Action::RecordingTitleSet {
                recording_id: recorded,
                title: "Private ordering duet title".into(),
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
        let synced = accepted(
            app,
            owner,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "member-order-http", "operations": operations,
            }),
        )
        .await;
        assert_eq!(
            synced["acknowledged"],
            json!(operations.iter().map(|op| op.id).collect::<Vec<_>>())
        );
        let metadata = owner
            .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
            .await;
        assert_eq!(metadata["title"]["personal"], "Private ordering duet title");
        assert!(metadata["artist"]["personal"].is_null());
        assert_eq!(metadata["credited_artists"].as_array().unwrap().len(), 2);
        let guest = &metadata["credited_artists"][1];
        assert_eq!(guest["role"], "featured");
        assert_eq!(guest["name"], MEMBER);
        let member = guest["artist_id"].as_str().unwrap().to_owned();
        Self {
            recording,
            member,
            playlist,
        }
    }

    pub(crate) async fn snapshot(&self, app: &Router, owner: &Account) -> Value {
        let metadata = owner
            .get(
                app,
                &format!("/api/v1/recordings/{}/metadata", self.recording),
            )
            .await;
        assert_eq!(metadata["title"]["personal"], "Private ordering duet title");
        assert!(metadata["artist"]["personal"].is_null());
        let sources = owner
            .get(
                app,
                &format!("/api/v1/recordings/{}/sources", self.recording),
            )
            .await;
        assert_eq!(sources, json!([]));
        let (status, download) = owner
            .request(
                app,
                "GET",
                &format!("/api/v1/recordings/{}/download", self.recording),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let progress = accepted(
            app,
            owner,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [self.recording]}),
        )
        .await;
        assert_eq!(progress[0]["position_ms"], 400);
        let recordings = owner.get(app, "/api/v1/recordings").await;
        assert_eq!(recordings["total"], 1);
        assert_eq!(recordings["items"][0]["liked"], true);
        assert_eq!(recordings["items"][0]["available"], false);
        json!({
            "recordings": recordings, "metadata": metadata, "sources": sources,
            "download": {"status": status.as_u16(), "body": download}, "progress": progress,
            "playlist": owner.get(app, &format!("/api/v1/playlists/{}", self.playlist)).await,
            "artists": owner.get(app, "/api/v1/artists").await,
            "releases": owner.get(app, "/api/v1/releases").await,
            "libraries": owner.get(app, "/api/v1/libraries").await,
            "activity": owner.get(app, "/api/v1/activity").await,
            "events": owner.get(app, "/api/v1/sync?cursor=0").await,
            "plugins": owner.get(app, "/api/v1/plugins").await,
            "policy": owner.get(app, "/api/v1/metadata/settings").await,
        })
    }
}
