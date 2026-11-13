use super::support;
use axum::{Json, http::StatusCode, routing::get};
use futures_util::FutureExt;
use hito_plugin_sdk::HealthReport;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::automatic_lrclib_fixture as lrclib;
use crate::{account, lyric_identity_fixture as library};

#[tokio::test]
async fn a_protected_credential_failure_overrides_cached_public_health_only_for_its_owner() {
    let upstream = lrclib::Upstream::start().await;
    let config = hito_plugin_lrclib::Config::new(lrclib::TOKEN)
        .unwrap()
        .with_endpoint(upstream.server.url.parse().unwrap())
        .unwrap();
    let shutdown = config.shutdown_handle();
    let health_reads = Arc::new(AtomicUsize::new(0));
    let observed = health_reads.clone();
    let router = hito_plugin_lrclib::router(config).unwrap().route(
        "/v1/health",
        get(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Json(HealthReport::ok()) }
        }),
    );
    let plugin = lrclib::Server::start(router).await;
    let result = AssertUnwindSafe(exercise(&plugin.url, &upstream, &health_reads))
        .catch_unwind()
        .await;
    shutdown.cancel();
    plugin.finish().await;
    upstream.server.finish().await;
    let installation = match result {
        Ok(installation) => installation,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation["health"]["state"], "credentials_required",
        "A protected credential rejection must supersede cached public health for its owner"
    );
}

async fn exercise(
    plugin_url: &str,
    upstream: &lrclib::Upstream,
    health_reads: &AtomicUsize,
) -> Value {
    let library = library::Library::prepare().await;
    let installation = library
        .account
        .connect(
            &library.app,
            plugin_url,
            "Owned public health service",
            None,
        )
        .await;
    let settings = library.account.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(settings["installations"][0]["health"]["state"], "ok");
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let (status, _) = library.account.request(
        &library.app, "POST", "/api/v1/users",
        json!({"username":"lyric-observation-peer","password":"owned observation peer password"}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &library.app,
        "lyric-observation-peer",
        "owned observation peer password",
    )
    .await;
    let (status, peer_installation) = peer.request(
        &library.app, "POST", "/api/v1/plugins",
        json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":lrclib::TOKEN}),
    ).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(installation["id"], peer_installation["id"]);
    let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
    assert_eq!(peer_before["installations"][0]["health"]["state"], "ok");
    assert_eq!(
        peer_before["installations"][0]["id"],
        peer_installation["id"]
    );
    let before = library.snapshot().await;
    let identities_path = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
    let identities = library.account.get(&library.app, &identities_path).await;
    assert_eq!(health_reads.load(Ordering::SeqCst), 2);
    let path = format!("/api/v1/recordings/{}/lyrics/candidates", library.recording);
    let request = json!({
        "installation_id":installation["id"],
        "installation_revision":installation["revision"],
        "query":format!("{} {}", lrclib::ARTIST, lrclib::TITLE),
    });
    let (status, unauthenticated) =
        support::call(&library.app, "POST", &path, request.clone(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(unauthenticated.get("plugin_connection").is_none());
    let (status, headers, response) = account::exchange(
        &library.app,
        "POST",
        &path,
        request,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response["code"], "plugin_credentials_required");
    assert_eq!(
        response["plugin_connection"],
        json!({
            "installation_id":installation["id"],
            "installation_revision":installation["revision"],
        })
    );
    let (status, headers, owner_after) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    let owner_installations = owner_after["installations"].as_array().unwrap();
    assert_eq!(owner_installations.len(), 1);
    assert_eq!(owner_installations[0]["id"], installation["id"]);
    assert_eq!(owner_installations[0]["revision"], installation["revision"]);
    assert_eq!(owner_installations[0]["credential_set"], false);
    assert_eq!(owner_after["services"], before["plugins"]["services"]);
    let (status, headers, peer_after) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        peer.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(peer_after, peer_before);
    assert_eq!(peer_after["installations"].as_array().unwrap().len(), 1);
    let mut after = library.snapshot().await;
    after["plugins"]["installations"][0]["health"] =
        before["plugins"]["installations"][0]["health"].clone();
    assert_eq!(after, before);
    assert_eq!(
        library.account.get(&library.app, &identities_path).await,
        identities
    );
    assert_eq!(health_reads.load(Ordering::SeqCst), 2);
    upstream.assert_calls(&[]);
    library.assert_original();
    owner_installations[0].clone()
}
