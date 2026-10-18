use crate::{
    artist_review_fixture::{self as fixture, Account, Imported, catalog},
    artist_review_host as host, artist_review_tcp as tcp,
    provider_fixture::ProviderServer,
};
use axum::{
    Router,
    http::{Method, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{ArtistCandidateSearchAvailability, ArtistReviewContext, MetadataProvider};
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest};
use serde_json::json;
use std::{
    net::SocketAddr,
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

#[tokio::test]
async fn a_hosted_artist_search_disconnect_retires_its_upstream_body_before_same_account_retry() {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "artist-review-disconnect-host").await;
    let app = hito_server::router(services.clone());
    let mut server = host::Host::start(services).await;
    let origin = ProviderServer::start().await;
    let mut provider = catalog::DiscoveryServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(&app, server.address, &origin, &provider)).catch_unwind(),
    )
    .await;
    provider.release_search();
    server.request_shutdown();
    let server_joined = server.join().await;
    origin.shutdown().await;
    let provider_joined = provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The owned hosted artist disconnect workflow must finish within its deadline")
        }
    }
    assert!(
        server_joined,
        "The owned Hito host must close and join shutdown"
    );
    assert!(
        provider_joined,
        "The owned catalog provider must close and join shutdown"
    );
}

async fn exercise(
    app: &Router,
    address: SocketAddr,
    origin: &ProviderServer,
    provider: &catalog::DiscoveryServer,
) {
    let account = Account::setup(app).await;
    let imported = Imported::create(app, &account, origin).await;
    let installation = account
        .connect(
            app,
            &provider.url,
            "Owned hosted artist catalog",
            Some(catalog::TOKEN),
        )
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
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let path = format!("/api/v1/recordings/{}/artist-review", imported.recording);
    let (status, context) = account
        .request(
            app,
            "POST",
            &format!("{path}/context"),
            json!({"provider": selected}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let context: ArtistReviewContext = serde_json::from_value(context).unwrap();
    assert!(context.account_id == account.id && context.artist_id == imported.artist);
    assert!(context.observation_id == imported.observation && context.source_id.is_none());
    let before = imported.snapshot(app, &account).await;
    let origin_before = fixture::origin_counts(origin).await;
    assert_eq!(origin_before, [1, 2, 1, 0, 0, 0, 0, 0]);
    assert_eq!(provider.requests().len(), 1);
    let search = ArtistSearchRequest {
        query: fixture::CREDIT.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };
    let body = json!({"context": context, "search": search});
    let gate = provider.hold_search();
    let socket = tcp::begin(address, &format!("{path}/search"), &body, account.headers()).await;
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("The real hosted request must receive the provider's first incomplete JSON byte");
    assert_eq!(provider.requests().len(), 3);
    drop(socket);
    tokio::time::timeout(Duration::from_secs(3), gate.retired())
        .await
        .expect("Dropping only the downstream Hito TCP client must retire the upstream body before release or shutdown");
    assert!(imported.snapshot(app, &account).await == before);
    assert_eq!(fixture::origin_counts(origin).await, origin_before);

    provider.release_search();
    let retry = tcp::begin(address, &format!("{path}/search"), &body, account.headers()).await;
    let (status, headers, found) = tcp::finish(retry).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            == Some("private, no-store")
    );
    let page: ArtistSearchPage = serde_json::from_value(found["page"].clone()).unwrap();
    page.validate_for(&search).unwrap();
    assert!(
        page.items.len() == 2
            && page.items[0].artist.id == catalog::FIRST_ARTIST
            && page.items[1].artist.id == catalog::SECOND_ARTIST
            && page.snapshot.as_deref() == Some(catalog::SNAPSHOT)
    );
    assert!(
        found
            == json!(ArtistCandidateSearchAvailability::Available {
                context: Box::new(context),
                page,
            })
    );
    assert!(
        imported.snapshot(app, &account).await == before,
        "A hosted read disconnect and retry must preserve private collection events, pins, duplicate entries, likes, progress and source/download absence"
    );
    for recording in [&imported.recording, &imported.other_recording] {
        assert!(
            account
                .get(
                    app,
                    &format!("/api/v1/recordings/{recording}/artist-associations")
                )
                .await
                == json!([])
        );
    }
    assert_eq!(fixture::origin_counts(origin).await, origin_before);
    let observed = provider.requests();
    assert_eq!(observed.len(), 5);
    assert!(observed[0].method == Method::GET && observed[0].path == "/prefix/v1/manifest");
    let (pairs, remainder) = observed[1..].as_chunks::<2>();
    assert!(remainder.is_empty());
    for pair in pairs {
        assert!(
            pair[0].method == Method::GET
                && pair[0].path == "/prefix/v1/catalog/discovery/capabilities"
                && pair[0].body.is_null()
        );
        assert!(
            pair[1].method == Method::POST
                && pair[1].path == "/prefix/v1/catalog/artists/search"
                && pair[1].body == json!(search)
        );
    }
    for request in observed {
        assert!(
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                == Some("Bearer private-artist-discovery-service-token")
        );
        assert!(request.headers.get("cookie").is_none());
    }
    assert_eq!(provider.unexpected(), 0);
}
