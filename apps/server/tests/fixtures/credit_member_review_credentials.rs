use crate::{
    account::{Account, exchange},
    credit_member_review_credentials_provider as provider,
    credit_member_review_owned::Library,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    Action, ArtistReviewContext, MetadataProvider, Operation,
    test_support::playlist_service as origin,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub struct Context {
    pub owner: Account,
    pub peer: Account,
    pub recording: String,
    pub member: String,
    pub context: Value,
    pub installation: Value,
    pub peer_installation: Value,
    playlist: String,
    before: Value,
    peer_before: Value,
}

impl Context {
    pub fn path(&self) -> String {
        format!(
            "/api/v1/recordings/{}/credit-members/{}/artist-review/search",
            self.recording, self.member
        )
    }

    pub async fn assert_private(&self, library: &Library) {
        assert_eq!(
            private_snapshot(library.app(), &self.owner, &self.recording, &self.playlist).await,
            self.before
        );
        assert_eq!(
            peer_snapshot(library.app(), &self.peer).await,
            self.peer_before
        );
    }

    pub async fn snapshot(&self, library: &Library) -> Value {
        json!({
            "owner": private_snapshot(library.app(), &self.owner, &self.recording, &self.playlist).await,
            "peer": peer_snapshot(library.app(), &self.peer).await,
        })
    }

    pub async fn fresh_context(&self, library: &Library) -> Value {
        accepted(
            library.app(),
            &self.owner,
            "POST",
            &format!(
                "/api/v1/recordings/{}/credit-members/{}/artist-review/context",
                self.recording, self.member
            ),
            json!({"provider": self.context["provider"]}),
        )
        .await
    }
}

pub async fn prepare(library: &Library) -> Context {
    let app = library.app();
    let origin = library.origin();
    let provider = library.plugin();
    let owner = Account::setup(app).await;
    origin.reply("v1/manifest", json!({
        "id": "org.hito.member-credentials-playlist", "name": "Owned member context playlist",
        "protocol_version": 1, "capabilities": ["playlists"]
    })).await;
    origin::playlist(origin, "owned-member-credentials", "owned-member-credentials:1", vec![json!({
        "occurrence_id": "owned-duet-appearance", "item": {"kind": "recording", "recording": {
            "id": "owned-member-credentials-duet", "title": "Owned duet", "artist": "Ada feat. Cy",
            "duration_ms": 100_000, "artists": [
                {"name": "Ada", "role": "main", "join": " feat. "},
                {"name": "Cy", "role": "featured"}
            ]
        }}
    })]).await;
    let origin_installation = owner
        .connect(app, &origin.url, "Owned member metadata", None)
        .await;
    let (status, receipt) = owner.request(app, "POST",
        &format!("/api/v1/plugins/{}/playlists/import", origin_installation["id"].as_str().unwrap()),
        json!({"request_id": Uuid::new_v4(), "installation_revision": origin_installation["revision"],
            "playlist_reference": "https://provider.invalid/member-credentials"}),
    ).await;
    assert_eq!(status, StatusCode::OK);
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
            title: "Private owned duet title".into(),
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
    let acknowledged = accepted(
        app,
        &owner,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "member-credentials-http", "operations": operations,
        }),
    )
    .await;
    assert_eq!(
        acknowledged["acknowledged"],
        json!(
            operations
                .iter()
                .map(|operation| operation.id)
                .collect::<Vec<_>>()
        )
    );
    let metadata = owner
        .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
        .await;
    assert!(metadata["artist"]["personal"].is_null());
    assert_eq!(metadata["credited_artists"].as_array().unwrap().len(), 2);
    let ada = &metadata["credited_artists"][0];
    let cy = &metadata["credited_artists"][1];
    assert_eq!(ada["name"], "Ada");
    assert_eq!(ada["role"], "main");
    assert_eq!(cy["name"], "Cy");
    assert_eq!(cy["role"], "featured");
    let member = cy["artist_id"].as_str().unwrap().to_owned();
    assert_ne!(ada["artist_id"], member);
    let installed = owner
        .connect(
            app,
            &provider.url,
            "Owned member artist catalog",
            Some(provider::OWNER_TOKEN),
        )
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installed["id"].as_str().unwrap().into(),
    };
    let settings = owner.get(app, "/api/v1/metadata/settings").await;
    let (status, policy) = owner
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let whole_context = accepted(
        app,
        &owner,
        "POST",
        &format!("/api/v1/recordings/{recording}/artist-review/context"),
        json!({"provider": selected}),
    )
    .await;
    let captured: ArtistReviewContext = serde_json::from_value(whole_context.clone()).unwrap();
    assert_eq!(captured.account_id, owner.id);
    assert_eq!(captured.recording_id, recording);
    assert_eq!(json!(captured.artist_id), ada["artist_id"]);
    assert_eq!(
        json!(captured.observation_id),
        metadata["selected_observation"]["id"]
    );
    assert_eq!(
        captured.policy_revision,
        policy["revision"].as_i64().unwrap()
    );
    assert!(captured.source_id.is_none() && captured.metadata_binding.is_some());
    let association = accepted(
        app,
        &owner,
        "POST",
        &format!("/api/v1/recordings/{recording}/artist-review/confirm"),
        json!({"context": whole_context, "provider_artist_id": "owned:artist/ada"}),
    )
    .await;
    assert_eq!(association["artist_id"], ada["artist_id"]);
    let chosen = accepted(
        app,
        &owner,
        "PUT",
        &format!("/api/v1/recordings/{recording}/artist-identity"),
        json!({
            "request_id": Uuid::new_v4(), "expected_revision": 0,
            "association_id": association["id"], "context": whole_context,
        }),
    )
    .await;
    assert_eq!(chosen["revision"], 1);
    assert_eq!(chosen["selection"]["association_id"], association["id"]);
    let (status, created) = owner.request(app, "POST", "/api/v1/users", json!({
        "username": "member-credentials-peer", "password": "owned member credentials peer password",
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        app,
        "member-credentials-peer",
        "owned member credentials peer password",
    )
    .await;
    assert_eq!(created["id"], peer.id);
    let expected = json!({
        "account_id": captured.account_id, "recording_id": captured.recording_id,
        "library_id": captured.library_id, "artist_id": member, "credited_name": "Cy", "role": "featured",
        "source_id": captured.source_id, "observation_id": captured.observation_id,
        "artist_operation_id": captured.artist_operation_id, "access_grant_id": captured.access_grant_id,
        "metadata_binding": captured.metadata_binding, "provider": selected,
        "policy_revision": captured.policy_revision, "connection": captured.connection,
    });
    let context = accepted(
        app,
        &owner,
        "POST",
        &format!("/api/v1/recordings/{recording}/credit-members/{member}/artist-review/context"),
        json!({"provider": selected}),
    )
    .await;
    assert_eq!(context, expected);
    let settings = owner.get(app, "/api/v1/plugins").await;
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installed["service_id"])
        .unwrap();
    let (status, peer_installation) = peer.request(app, "POST", "/api/v1/plugins", json!({
        "service_id": service["id"], "service_revision": service["revision"], "bearer_token": provider::PEER_TOKEN,
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_installation["id"], installed["id"]);
    let before = private_snapshot(app, &owner, &recording, &playlist).await;
    let peer_before = peer_snapshot(app, &peer).await;
    Context {
        owner,
        peer,
        recording,
        member,
        playlist,
        installation: installed,
        peer_installation,
        context,
        before,
        peer_before,
    }
}

async fn private_snapshot(app: &Router, owner: &Account, recording: &str, playlist: &str) -> Value {
    let (status, progress) = owner
        .request(
            app,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [recording]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    json!({
        "recordings": owner.get(app, "/api/v1/recordings").await,
        "metadata": owner.get(app, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources": owner.get(app, &format!("/api/v1/recordings/{recording}/sources")).await,
        "playlist": owner.get(app, &format!("/api/v1/playlists/{playlist}")).await,
        "choice": owner.get(app, &format!("/api/v1/recordings/{recording}/artist-identity")).await,
        "associations": owner.get(app, &format!("/api/v1/recordings/{recording}/artist-associations")).await,
        "groups": owner.get(app, "/api/v1/artist-identity-groups").await,
        "events": owner.get(app, "/api/v1/sync?cursor=0").await,
        "settings": owner.get(app, "/api/v1/metadata/settings").await,
        "progress": progress,
    })
}

async fn peer_snapshot(app: &Router, peer: &Account) -> Value {
    json!({
        "recordings": peer.get(app, "/api/v1/recordings").await,
        "playlists": peer.get(app, "/api/v1/playlists").await,
        "groups": peer.get(app, "/api/v1/artist-identity-groups").await,
        "plugins": peer.get(app, "/api/v1/plugins").await,
        "settings": peer.get(app, "/api/v1/metadata/settings").await,
        "events": peer.get(app, "/api/v1/sync?cursor=0").await,
    })
}

pub fn installation(settings: &Value, id: &Value) -> Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|installation| installation["id"] == *id)
        .unwrap()
        .clone()
}

pub async fn accepted(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
) -> Value {
    let (status, headers, value) = exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    value
}

pub fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
