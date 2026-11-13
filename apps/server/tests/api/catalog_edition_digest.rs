use crate::{
    album_editions::{Prepared, prepare},
    artist_review_fixture::{self as review, Account},
    support,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    Action, CoreConfig, HostDeviceId, Operation,
    test_support::{
        album_edition_service::{self as catalog, AlbumProvider},
        database_owner::DatabaseOwner,
        hang_guard::hang_guard,
    },
};
use hito_plugin_sdk::{CatalogEditionTrackList, EditionTrackListRequest};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

const EDITION_DIGEST: &str = "ce9caa59b2ef317a582ae0d58b8f251e48e5d6ceaa8eee5b1d38d48ea29a0093";

#[tokio::test]
async fn a_selected_catalog_edition_exposes_its_content_digest_without_mutating_the_library() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-edition-digest-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("edition-digest-http").unwrap(),
    );
    owner.attach(&mut config);
    let mut services: Option<hito_core::Services> = None;
    let mut provider: Option<AlbumProvider> = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        services = Some(
            hang_guard(
                "edition digest owned services",
                Box::pin(hito_core::open(config)),
            )
            .await
            .unwrap(),
        );
        let app = hito_server::router(services.as_ref().unwrap().clone());
        provider = Some(
            hang_guard(
                "edition digest owned catalog",
                Box::pin(AlbumProvider::start()),
            )
            .await,
        );
        let catalog = provider.as_ref().unwrap();
        let account = hang_guard("edition digest account", Box::pin(Account::setup(&app))).await;
        let prepared = hang_guard(
            "edition digest reviewed catalog",
            Box::pin(prepare(&app, &account, catalog)),
        )
        .await;
        let peer = hang_guard(
            "edition digest private state",
            Box::pin(private_state(&app, &account, &prepared)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            read_catalog_edition(&app, &account, &peer, &prepared, catalog),
        )
        .await
        .expect("The catalog edition digest read must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    let joined = match provider.as_mut() {
        Some(provider) => provider.finish().await,
        None => true,
    };
    let [playback_closed, replicas_closed, replication_closed] =
        if let Some(services) = services.take() {
            let playback = services.playback.close().await;
            let replicas = services.replicas.close().await;
            let replication = services.catalog_replication.close().await;
            drop(services);
            [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
        } else {
            [true; 3]
        };
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(joined);
    assert!(playback_closed && replicas_closed && replication_closed);
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert_eq!(std::fs::read(audio).unwrap(), original);
    let response = match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        response.get("edition_digest"),
        Some(&json!(EDITION_DIGEST)),
        "The exact selected edition must carry a Core-issued content digest, separate from account authority"
    );
}

async fn private_state(app: &Router, account: &Account, prepared: &Prepared) -> Account {
    let first = Uuid::parse_str(&prepared.first).unwrap();
    let second = Uuid::parse_str(prepared.saved[1]["id"].as_str().unwrap()).unwrap();
    let operations = [
        Action::RecordingTitleSet {
            recording_id: second,
            title: "Owned private digest title".into(),
        },
        Action::LikeSet {
            recording_id: first,
            liked: true,
        },
        Action::ProgressSet {
            recording_id: first,
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
                "device_id": "owned-edition-digest", "operations": operations,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        accepted["acknowledged"],
        json!(
            operations
                .iter()
                .map(|operation| operation.id)
                .collect::<Vec<_>>()
        )
    );
    let (status, _) = account
        .request(
            app,
            "POST",
            "/api/v1/users",
            json!({
                "username": "edition-digest-peer", "password": "owned edition digest peer password",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    Account::login(
        app,
        "edition-digest-peer",
        "owned edition digest peer password",
    )
    .await
}

async fn read_catalog_edition(
    app: &Router,
    account: &Account,
    peer: &Account,
    prepared: &Prepared,
    provider: &AlbumProvider,
) -> Value {
    let expected_edition = catalog::track_list(catalog::EDITION_ID).unwrap();
    serde_json::from_value::<CatalogEditionTrackList>(expected_edition.clone())
        .unwrap()
        .validate_for(&EditionTrackListRequest {
            release_group_id: catalog::GROUP_ID.into(),
            edition_id: catalog::EDITION_ID.into(),
        })
        .unwrap();
    let before = snapshot(app, account, prepared).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let peer_plugins = peer.get(app, "/api/v1/plugins").await;
    let requests_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}/edition-track-list",
        prepared.first, prepared.association
    );
    let body = json!({"release_group_id": catalog::GROUP_ID, "edition_id": catalog::EDITION_ID});
    let (status, headers, _) =
        review::exchange(app, "POST", &path, body.clone(), peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(provider.requests().len(), requests_before);
    let (status, headers, response) =
        review::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(response["status"], "available");
    assert_eq!(response["association_id"], prepared.association);
    assert_eq!(response["edition"], expected_edition);
    assert_eq!(response["linked"], json!([]));
    assert_eq!(snapshot(app, account, prepared).await, before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(peer.get(app, "/api/v1/plugins").await, peer_plugins);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(dispatched.len(), 2);
    assert_eq!(
        dispatched[0].path,
        "/prefix/v1/catalog/editions/capabilities"
    );
    assert_eq!(dispatched[1].path, "/prefix/v1/catalog/editions/track-list");
    assert_eq!(dispatched[1].body, body);
    assert!(
        dispatched
            .iter()
            .all(|request| request.headers["x-hito-plugin-protocol"] == "1")
    );
    response
}

async fn snapshot(app: &Router, account: &Account, prepared: &Prepared) -> Value {
    let mut metadata = Vec::new();
    for saved in &prepared.saved {
        let recording = saved["id"].as_str().unwrap();
        metadata.push(
            account
                .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
                .await,
        );
        assert_eq!(
            account
                .get(app, &format!("/api/v1/recordings/{recording}/sources"))
                .await,
            json!([])
        );
        let (status, _) = account
            .request(
                app,
                "GET",
                &format!("/api/v1/recordings/{recording}/download"),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    json!({
        "album": account.get(app, &format!("/api/v1/releases/{}", prepared.release)).await,
        "catalog": account.get(app, "/api/v1/recordings").await,
        "releases": account.get(app, "/api/v1/releases").await,
        "artists": account.get(app, "/api/v1/artists").await,
        "libraries": account.get(app, "/api/v1/libraries").await,
        "activity": account.get(app, "/api/v1/activity").await,
        "events": account.get(app, "/api/v1/sync?cursor=0").await,
        "plugins": account.get(app, "/api/v1/plugins").await,
        "policy": account.get(app, "/api/v1/metadata/settings").await,
        "metadata": metadata,
    })
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
