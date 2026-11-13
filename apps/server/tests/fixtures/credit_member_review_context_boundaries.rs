pub(crate) use crate::account::Account;
use crate::{account::exchange, artist_review_fixture::origin_counts};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
pub(crate) use hito_core::test_support::{
    artist_catalog_discography_service as catalog, playlist_service as origin,
};
use hito_core::{
    Action, ArtistReviewContext, CoreConfig, HostDeviceId, MetadataProvider, Operation, Services,
    test_support::database_owner::DatabaseOwner,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) struct Library {
    _directory: Arc<tempfile::TempDir>,
    database: DatabaseOwner,
    config: Option<CoreConfig>,
    services: Option<Services>,
    pub(crate) origin: Option<origin::ProviderServer>,
    pub(crate) provider: Option<catalog::CatalogServer>,
}

impl Library {
    pub(crate) fn new() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let database = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("member-boundaries-http").unwrap(),
        );
        database.attach(&mut config);
        Self {
            _directory: directory,
            database,
            config: Some(config),
            services: None,
            origin: None,
            provider: None,
        }
    }

    pub(crate) async fn prepare(&mut self) -> (Router, Prepared) {
        self.services = Some(hito_core::open(self.config.take().unwrap()).await.unwrap());
        self.origin = Some(origin::ProviderServer::start().await);
        self.provider = Some(catalog::CatalogServer::start().await);
        let app = hito_server::router(self.services.as_ref().unwrap().clone());
        let prepared = Box::pin(prepare(
            &app,
            self.origin.as_ref().unwrap(),
            self.provider.as_ref().unwrap(),
        ))
        .await;
        (app, prepared)
    }

    pub(crate) async fn retire(&mut self) {
        if let Some(origin) = self.origin.take() {
            origin.shutdown().await;
        }
        let provider_joined = match self.provider.as_mut() {
            Some(provider) => provider.finish().await,
            None => true,
        };
        let closed = if let Some(hito) = self.services.take() {
            let playback = hito.playback.close().await;
            let replicas = hito.replicas.close().await;
            let replication = hito.catalog_replication.close().await;
            drop(hito);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
        let retired = self.database.retire(Duration::from_secs(5)).await;
        assert!(
            provider_joined,
            "The owned catalog provider must join shutdown"
        );
        assert!(closed.into_iter().all(|closed| closed));
        assert!(retired.resources_retired && !retired.deadline_exceeded);
        assert_eq!(self.database.observe().active_resources(), 0);
    }
}

pub(crate) struct Selection {
    pub(crate) recording: String,
    playlist: String,
    metadata: Value,
}

pub(crate) struct Prepared {
    pub(crate) owner: Account,
    pub(crate) peer: Account,
    pub(crate) owner_selection: Selection,
    pub(crate) peer_selection: Selection,
    pub(crate) main: String,
    pub(crate) foreign: String,
    pub(crate) installation: Value,
    pub(crate) service_revision: Value,
    pub(crate) provider: MetadataProvider,
    pub(crate) expected: Value,
}

async fn prepare(
    app: &Router,
    origin: &origin::ProviderServer,
    provider: &catalog::CatalogServer,
) -> Prepared {
    let owner = Account::setup(app).await;
    origin.reply("v1/manifest", json!({
        "id": "org.hito.member-boundaries-playlist", "name": "Owned member boundaries playlist",
        "protocol_version": 1, "capabilities": ["playlists"]
    })).await;
    origin::playlist(origin, "owned-member-boundaries", "owned-member-boundaries:1", vec![json!({
        "occurrence_id": "owned-trio-appearance", "item": {"kind": "recording", "recording": {
            "id": "owned-member-boundaries-trio", "title": "Owned trio", "artist": "Ada & Bea feat. Cy",
            "duration_ms": 100_000, "artists": [
                {"name": "Ada", "role": "main", "join": " & "},
                {"name": "Bea", "role": "main", "join": " feat. "},
                {"name": "Cy", "role": "featured"}
            ]
        }}
    })]).await;
    let origin_installation = owner
        .connect(app, &origin.url, "Owned member boundary metadata", None)
        .await;
    let owner_selection = Box::pin(import(app, &owner, &origin_installation)).await;
    let recording = &owner_selection.recording;
    let main = owner_selection.metadata["credited_artists"][0]["artist_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let recorded = Uuid::parse_str(recording).unwrap();
    let operations = [
        Action::RecordingTitleSet {
            recording_id: recorded,
            title: "Private owned trio title".into(),
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
            "device_id": "member-boundaries-http", "operations": operations,
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
    let installation = owner
        .connect(app, &provider.url, "Owned member boundary catalog", None)
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
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
    assert_eq!(&captured.recording_id, recording);
    assert_ne!(captured.artist_id, main);
    assert_eq!(
        json!(captured.observation_id),
        owner_selection.metadata["selected_observation"]["id"]
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
        json!({"context": whole_context, "provider_artist_id": "owned:artist/whole-trio"}),
    )
    .await;
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
        "username": "member-boundary-peer", "password": "owned member boundary peer password",
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        app,
        "member-boundary-peer",
        "owned member boundary peer password",
    )
    .await;
    assert_eq!(created["id"], peer.id);
    let services = owner.get(app, "/api/v1/plugins").await;
    let offered = services["services"].as_array().unwrap();
    let origin_revision = offered
        .iter()
        .find(|service| service["id"] == origin_installation["service_id"])
        .unwrap()["revision"]
        .clone();
    let service_revision = offered
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap()["revision"]
        .clone();
    let (status, peer_installation) = peer.request(app, "POST", "/api/v1/plugins", json!({
        "service_id": origin_installation["service_id"], "service_revision": origin_revision, "bearer_token": null,
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer_selection = Box::pin(import(app, &peer, &peer_installation)).await;
    assert_ne!(peer_selection.recording, owner_selection.recording);
    let foreign = peer_selection.metadata["credited_artists"][0]["artist_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(foreign, main);
    let expected = json!({
        "account_id": captured.account_id, "recording_id": captured.recording_id,
        "library_id": captured.library_id, "artist_id": main, "credited_name": "Ada", "role": "main",
        "source_id": captured.source_id, "observation_id": captured.observation_id,
        "artist_operation_id": captured.artist_operation_id, "access_grant_id": captured.access_grant_id,
        "metadata_binding": captured.metadata_binding, "provider": selected,
        "policy_revision": captured.policy_revision, "connection": captured.connection,
    });
    Prepared {
        owner,
        peer,
        owner_selection,
        peer_selection,
        main,
        foreign,
        installation,
        service_revision,
        provider: selected,
        expected,
    }
}

async fn import(app: &Router, account: &Account, installation: &Value) -> Selection {
    let (status, receipt) = account
        .request(
            app,
            "POST",
            &format!(
                "/api/v1/plugins/{}/playlists/import",
                installation["id"].as_str().unwrap()
            ),
            json!({"request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/member-boundaries"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
    let copy = account
        .get(app, &format!("/api/v1/playlists/{playlist}"))
        .await;
    assert_eq!(copy["entries"].as_array().unwrap().len(), 1);
    let recording = copy["entries"][0]["recording_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let metadata = account
        .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
        .await;
    assert!(metadata["artist"]["personal"].is_null());
    let members = metadata["credited_artists"].as_array().unwrap();
    assert_eq!(members.len(), 3);
    for (member, (name, role)) in
        members
            .iter()
            .zip([("Ada", "main"), ("Bea", "main"), ("Cy", "featured")])
    {
        assert_eq!(member["name"], name);
        assert_eq!(member["role"], role);
        assert!(member["artist_id"].is_string());
    }
    Selection {
        recording,
        playlist,
        metadata,
    }
}

pub(crate) async fn preserved_context(
    app: &Router,
    prepared: &Prepared,
    origin: &origin::ProviderServer,
    provider: &catalog::CatalogServer,
    account: &Account,
    path: &str,
    selected: &MetadataProvider,
) -> (StatusCode, HeaderMap, Value) {
    let owner_before = snapshot(app, &prepared.owner, &prepared.owner_selection).await;
    let peer_before = snapshot(app, &prepared.peer, &prepared.peer_selection).await;
    let owner_plugins = prepared.owner.get(app, "/api/v1/plugins").await;
    let peer_plugins = prepared.peer.get(app, "/api/v1/plugins").await;
    let settings = prepared.owner.get(app, "/api/v1/metadata/settings").await;
    let peer_settings = prepared.peer.get(app, "/api/v1/metadata/settings").await;
    let requests = provider.requests().len();
    let origins = origin_counts(origin).await;
    let response = exchange(
        app,
        "POST",
        path,
        json!({"provider": selected}),
        account.headers(),
    )
    .await;
    assert_eq!(
        snapshot(app, &prepared.owner, &prepared.owner_selection).await,
        owner_before
    );
    assert_eq!(
        snapshot(app, &prepared.peer, &prepared.peer_selection).await,
        peer_before
    );
    assert_eq!(
        prepared.owner.get(app, "/api/v1/plugins").await,
        owner_plugins
    );
    assert_eq!(
        prepared.peer.get(app, "/api/v1/plugins").await,
        peer_plugins
    );
    assert_eq!(
        prepared.owner.get(app, "/api/v1/metadata/settings").await,
        settings
    );
    assert_eq!(
        prepared.peer.get(app, "/api/v1/metadata/settings").await,
        peer_settings
    );
    assert_eq!(
        provider.requests().len(),
        requests,
        "Context capture or denial must not contact the catalog provider"
    );
    assert_eq!(origin_counts(origin).await, origins);
    assert_eq!(provider.unexpected(), 0);
    response
}

pub(crate) async fn snapshot(app: &Router, account: &Account, selection: &Selection) -> Value {
    let recording = &selection.recording;
    let (status, progress) = account
        .request(
            app,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [recording]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    json!({
        "recordings": account.get(app, "/api/v1/recordings").await,
        "metadata": account.get(app, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources": account.get(app, &format!("/api/v1/recordings/{recording}/sources")).await,
        "playlist": account.get(app, &format!("/api/v1/playlists/{}", selection.playlist)).await,
        "choice": account.get(app, &format!("/api/v1/recordings/{recording}/artist-identity")).await,
        "associations": account.get(app, &format!("/api/v1/recordings/{recording}/artist-associations")).await,
        "groups": account.get(app, "/api/v1/artist-identity-groups").await,
        "events": account.get(app, "/api/v1/sync?cursor=0").await,
        "progress": progress,
    })
}

pub(crate) async fn accepted(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
) -> Value {
    let (status, headers, body) = exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    body
}

pub(crate) fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
