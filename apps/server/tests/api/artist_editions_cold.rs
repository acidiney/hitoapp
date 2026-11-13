use crate::{
    artist_review_fixture::{self as fixture, Account, Imported},
    provider_fixture::ProviderServer,
    support,
};
use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    ArtistReviewContext, MetadataProvider, ReleaseGroupEditionsAvailability,
    test_support::{artist_catalog_editions_service as catalog, hang_guard::hang_guard},
};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn a_cold_release_group_lists_editions_without_prior_discography() {
    let directory = tempfile::tempdir().unwrap();
    let hito = hang_guard(
        "cold edition HTTP services",
        Box::pin(support::services(
            directory.path(),
            &directory.path().join("music"),
        )),
    )
    .await;
    let app = hito_server::router(hito.clone());
    let origin = ProviderServer::start().await;
    let mut provider = catalog::CatalogServer::start().await;
    provider.enable_edition_listing();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let (owner, peer, imported, association) = hang_guard(
            "cold edition account and reviewed association",
            Box::pin(prepare(&app, &origin, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(
                &app,
                &owner,
                &peer,
                &imported,
                &association,
                &origin,
                &provider,
            )),
        )
        .await
        .expect("The cold edition HTTP workflow must finish within its deadline");
    }))
    .catch_unwind()
    .await;
    origin.shutdown().await;
    let joined = provider.finish().await;
    let closed = hito.catalog_replication.close().await;
    drop(app);
    drop(hito);
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
    assert!(joined, "The owned catalog provider must join shutdown");
    assert!(closed.is_ok());
}

async fn prepare(
    app: &Router,
    origin: &ProviderServer,
    provider: &catalog::CatalogServer,
) -> (Account, Account, Imported, Value) {
    let owner = Account::setup(app).await;
    let imported = Imported::create(app, &owner, origin).await;
    let installation = owner
        .connect(app, &provider.url, "Owned cold edition catalog", None)
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
    };
    let settings = owner.get(app, "/api/v1/metadata/settings").await;
    let (status, _) = owner
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
        &owner,
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let captured: ArtistReviewContext = serde_json::from_value(context.clone()).unwrap();
    assert_eq!(captured.account_id, owner.id);
    assert_eq!(captured.recording_id, imported.recording);
    assert_eq!(captured.artist_id, imported.artist);
    assert_eq!(captured.observation_id, imported.observation);
    assert_eq!(captured.provider, selected);
    assert!(captured.source_id.is_none() && captured.metadata_binding.is_some());
    let choice = json!({"context": context, "provider_artist_id": catalog::ARTIST_ID});
    let preview = accepted(app, &owner, &format!("{review}/preview"), choice.clone()).await;
    assert_eq!(preview["status"], "available");
    assert_eq!(preview["profile"]["artist"]["id"], catalog::ARTIST_ID);
    let association = accepted(app, &owner, &format!("{review}/confirm"), choice).await;
    assert_eq!(association["account_id"], owner.id);
    assert_eq!(association["recording_id"], imported.recording);
    assert_eq!(association["provider"], json!(selected));
    assert_eq!(association["provider_artist_id"], catalog::ARTIST_ID);
    assert!(Uuid::parse_str(association["id"].as_str().unwrap()).is_ok());
    let (status, _) = owner
        .request(
            app,
            "POST",
            "/api/v1/users",
            json!({"username": "peer", "password": "owned cold edition peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(app, "peer", "owned cold edition peer password").await;
    (owner, peer, imported, association)
}

async fn exercise(
    app: &Router,
    owner: &Account,
    peer: &Account,
    imported: &Imported,
    association: &Value,
    origin: &ProviderServer,
    provider: &catalog::CatalogServer,
) {
    let before = imported.snapshot(app, owner).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let peer_plugins = peer.get(app, "/api/v1/plugins").await;
    assert_eq!(peer_plugins["installations"], json!([]));
    let origin_before = fixture::origin_counts(origin).await;
    assert_eq!(origin_before, [1, 2, 1, 0, 0, 0, 0, 0]);
    let requests_before = provider.requests();
    assert!(requests_before.iter().all(|request| matches!(
        request.path.as_str(),
        "/prefix/v1/manifest"
            | "/prefix/v1/catalog/capabilities"
            | "/prefix/v1/catalog/artists/profile"
    )));
    let associations = format!(
        "/api/v1/recordings/{}/artist-associations",
        imported.recording
    );
    let path = format!(
        "{associations}/{}/release-group-editions",
        association["id"].as_str().unwrap()
    );
    let body = json!({"release_group_id": catalog::GROUP_ID, "limit": 1, "continuation": null});
    let (status, headers, _) =
        fixture::exchange(app, "POST", &path, body.clone(), peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(
        provider.requests(),
        requests_before,
        "Peer denial must precede provider dispatch"
    );
    let (status, headers, response) =
        fixture::exchange(app, "POST", &path, body, owner.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    let continuation = response["continuation"].as_str().unwrap().to_owned();
    assert!(continuation.is_ascii() && !continuation.is_empty() && continuation.len() <= 8192);
    assert_ne!(continuation, catalog::PROVIDER_CURSOR);
    assert_eq!(
        response,
        json!(ReleaseGroupEditionsAvailability::Available {
            association_id: association["id"].as_str().unwrap().into(),
            page: catalog::expected_editions(false),
            continuation: Some(continuation),
        })
    );
    let requests = provider.requests();
    let dispatched = requests[requests_before.len()..]
        .iter()
        .map(|request| {
            (
                request.method.clone(),
                request.path.as_str(),
                request.body.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dispatched,
        vec![
            (
                Method::GET,
                "/prefix/v1/catalog/editions/capabilities",
                Value::Null
            ),
            (
                Method::POST,
                "/prefix/v1/catalog/release-groups/editions",
                json!({
                    "release_group_id": catalog::GROUP_ID, "cursor": null,
                    "snapshot": null, "limit": 1,
                }),
            ),
        ]
    );
    assert!(
        requests
            .iter()
            .all(|request| request.path != "/prefix/v1/catalog/artists/discography")
    );
    assert_eq!(provider.unexpected(), 0);
    assert_eq!(imported.snapshot(app, owner).await, before);
    assert_eq!(owner.get(app, &associations).await, json!([association]));
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(peer.get(app, "/api/v1/plugins").await, peer_plugins);
    assert_eq!(fixture::origin_counts(origin).await, origin_before);
}

async fn accepted(app: &Router, account: &Account, path: &str, body: Value) -> Value {
    let (status, headers, value) =
        fixture::exchange(app, "POST", path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    value
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
    assert!(Uuid::parse_str(headers.get("x-request-id").unwrap().to_str().unwrap()).is_ok());
}
