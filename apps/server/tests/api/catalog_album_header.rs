use crate::{
    artist_review_fixture::{self as review, Account, Imported},
    catalog_album_header_fixture as catalog,
    provider_fixture::ProviderServer,
    support,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::{
    CoreConfig, HostDeviceId, MetadataProvider,
    test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard},
};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

struct Reviewed {
    account: Account,
    peer: Account,
    imported: Imported,
    association: Value,
}

#[tokio::test]
async fn a_cold_album_header_reads_its_canonical_group_without_discography_or_audio() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-header-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("cold-header-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "cold header owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let origin = hang_guard(
        "cold header owned origin",
        Box::pin(ProviderServer::start()),
    )
    .await;
    let mut provider = hang_guard("cold header owned catalog", Box::pin(catalog::start())).await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let reviewed = hang_guard(
            "cold header reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(&app, &reviewed, &origin, &provider),
        )
        .await
        .expect("The cold header workflow must finish within its deadline");
    }))
    .catch_unwind()
    .await;
    let joined = provider.finish().await;
    origin.shutdown().await;
    let playback_closed = services.playback.close().await;
    let replicas_closed = services.replicas.close().await;
    let replication_closed = services.catalog_replication.close().await;
    drop(app);
    drop(services);
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(joined);
    assert!(playback_closed.is_ok() && replicas_closed.is_ok() && replication_closed.is_ok());
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert_eq!(std::fs::read(audio).unwrap(), original);
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn prepare(app: &Router, origin: &ProviderServer, provider: &FakePlugin) -> Reviewed {
    let account = Account::setup(app).await;
    let imported = Imported::create(app, &account, origin).await;
    let installation = account
        .connect(app, &provider.url, "Owned header catalog", None)
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
    };
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    let (status, _) = account
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({"revision": settings["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let review = format!("/api/v1/recordings/{}/artist-review", imported.recording);
    let context = accepted(
        app,
        &account,
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let decision = json!({"context": context, "provider_artist_id": catalog::ARTIST});
    let preview = accepted(
        app,
        &account,
        &format!("{review}/preview"),
        decision.clone(),
    )
    .await;
    assert_eq!(preview["status"], "available");
    assert_eq!(preview["profile"]["artist"]["id"], catalog::ARTIST);
    let association = accepted(app, &account, &format!("{review}/confirm"), decision).await;
    assert_eq!(association["recording_id"], imported.recording);
    assert_eq!(association["provider_artist_id"], catalog::ARTIST);
    let (status, _) = account
        .request(
            app,
            "POST",
            "/api/v1/users",
            json!({"username": "header-peer", "password": "owned header peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(app, "header-peer", "owned header peer password").await;
    Reviewed {
        account,
        peer,
        imported,
        association,
    }
}

async fn exercise(
    app: &Router,
    reviewed: &Reviewed,
    origin: &ProviderServer,
    provider: &FakePlugin,
) {
    let Reviewed {
        account,
        peer,
        imported,
        association,
    } = reviewed;
    let before = imported.snapshot(app, account).await;
    let origin_before = review::origin_counts(origin).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let requests_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}/release-group",
        imported.recording,
        association["id"].as_str().unwrap(),
    );
    let body = json!({"release_group_id": catalog::GROUP});
    let (status, headers, _) =
        review::exchange(app, "POST", &path, body.clone(), peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(provider.requests().len(), requests_before);
    let (status, headers, response) =
        review::exchange(app, "POST", &path, body, account.headers()).await;
    private(&headers);
    assert_eq!(imported.snapshot(app, account).await, before);
    assert_eq!(review::origin_counts(origin).await, origin_before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(provider.unexpected(), 0);
    assert_eq!(
        status,
        StatusCode::OK,
        "The targeted release-group read must be available without a prior discography visit"
    );
    assert_eq!(
        response,
        json!({
            "status": "available", "association_id": association["id"], "group": catalog::group(),
        })
    );
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[0].operation, catalog::GROUP_CAPABILITIES);
    assert_eq!(dispatched[1].operation, catalog::GROUP_DETAIL);
    assert_eq!(
        dispatched[1].json(),
        json!({"release_group_id": catalog::GROUP})
    );
}

async fn accepted(app: &Router, account: &Account, path: &str, body: Value) -> Value {
    let (status, value) = account.request(app, "POST", path, body).await;
    assert_eq!(status, StatusCode::OK);
    value
}

fn private(headers: &axum::http::HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}

#[tokio::test]
async fn an_old_catalog_without_group_lookup_keeps_its_explicit_edition_usable() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-legacy-header-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("legacy-header-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "legacy header owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let origin = hang_guard(
        "legacy header owned origin",
        Box::pin(ProviderServer::start()),
    )
    .await;
    let mut provider = hang_guard("legacy header owned catalog", Box::pin(catalog::start())).await;
    provider.reply(
        catalog::GROUP_CAPABILITIES,
        hito_testkit::fake_plugin::Reply::Status(StatusCode::NOT_FOUND),
    );
    let outcome = AssertUnwindSafe(Box::pin(async {
        let reviewed = hang_guard(
            "legacy header reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            legacy_edition(&app, &reviewed, &origin, &provider),
        )
        .await
        .expect("The legacy catalog workflow must finish within its deadline");
    }))
    .catch_unwind()
    .await;
    let joined = provider.finish().await;
    origin.shutdown().await;
    let playback_closed = services.playback.close().await;
    let replicas_closed = services.replicas.close().await;
    let replication_closed = services.catalog_replication.close().await;
    drop(app);
    drop(services);
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(joined);
    assert!(playback_closed.is_ok() && replicas_closed.is_ok() && replication_closed.is_ok());
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert_eq!(std::fs::read(audio).unwrap(), original);
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn legacy_edition(
    app: &Router,
    reviewed: &Reviewed,
    origin: &ProviderServer,
    provider: &FakePlugin,
) {
    let Reviewed {
        account,
        peer,
        imported,
        association,
    } = reviewed;
    let before = imported.snapshot(app, account).await;
    let origin_before = review::origin_counts(origin).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let requests_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}",
        imported.recording,
        association["id"].as_str().unwrap(),
    );
    let (status, headers, unavailable) = review::exchange(
        app,
        "POST",
        &format!("{path}/release-group"),
        json!({"release_group_id": catalog::GROUP}),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(unavailable, json!({"status": "unsupported"}));
    assert_eq!(imported.snapshot(app, account).await, before);
    let (status, headers, tracks) = review::exchange(
        app,
        "POST",
        &format!("{path}/edition-track-list"),
        json!({"release_group_id": catalog::GROUP, "edition_id": catalog::EDITION}),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(tracks["status"], "available");
    assert_eq!(tracks["association_id"], association["id"]);
    assert_eq!(tracks["edition"], json!(catalog::edition()));
    assert_eq!(tracks["linked"], json!([]));
    assert_eq!(imported.snapshot(app, account).await, before);
    assert_eq!(review::origin_counts(origin).await, origin_before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(
        dispatched
            .iter()
            .map(|request| request.operation.as_str())
            .collect::<Vec<_>>(),
        [
            catalog::GROUP_CAPABILITIES,
            "v1/catalog/editions/capabilities",
            catalog::TRACK_LIST,
        ]
    );
    assert_eq!(
        dispatched[2].json(),
        json!({
            "release_group_id": catalog::GROUP, "edition_id": catalog::EDITION,
        })
    );
}

#[path = "catalog_album_header_parallel.rs"]
mod parallel;


#[path = "catalog_album_header_stats.rs"]
mod stats;

