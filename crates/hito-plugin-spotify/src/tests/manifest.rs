use super::Server;
use crate::{Config, router};
use axum::{Router, http::StatusCode};
use hito_plugin_sdk::{Capability, Manifest};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn manifest_declares_only_public_playlists_without_authorization_or_upstream_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let upstream = Server::start(Router::new().fallback(move || {
        let observed = observed.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }))
    .await;
    let plugin = Server::start(
        router(Config {
            api_base: upstream.url.join("v1/").unwrap(),
            public_base: upstream.url.clone(),
            query_endpoint: upstream.url.join("pathfinder/v2/query").unwrap(),
        })
        .unwrap(),
    )
    .await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .get(plugin.url.join("v1/manifest").unwrap())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let value = response.json::<serde_json::Value>().await.unwrap();
    let manifest = serde_json::from_value::<Manifest>(value.clone()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.id, "org.hito.spotify");
    assert_eq!(manifest.name, "Spotify public playlists");
    assert_eq!(manifest.protocol_version, 1);
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from([Capability::Playlists])
    );
    assert!(manifest.authorization.is_none());
    assert!(value.get("authorization").is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
