use crate::{
    assert_private_logs,
    fixture::{self, Account, Library},
    logs, provider,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::{Action, Operation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const PEER_NAME: &str = "Owned private \"peer lyric\" playlist";
const PEER_PASSWORD: &str = "owned private lyric peer password";

#[tokio::test]
async fn foreign_and_stale_lyric_authority_is_rejected_before_upstream_work() {
    let provider = provider::Provider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(authority(&provider)).catch_unwind(),
    )
    .await;
    provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned lyric authority workflow must finish within its deadline"),
    }
}

async fn authority(provider: &provider::Provider) {
    let library = Library::prepare(provider).await;
    let (peer, playlist) = peer(&library).await;
    let before = library.snapshot().await;
    let peer_before = peer_snapshot(&library.app, &peer, &playlist).await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber());
    let mut anonymous = library.account.headers();
    anonymous.remove("cookie");
    let mut foreign_account = library.account.headers();
    foreign_account.insert("x-hito-account", peer.id.parse().unwrap());
    let mut foreign_origin = library.account.headers();
    foreign_origin.insert("origin", "https://untrusted.example".parse().unwrap());
    let mut missing_guard = library.account.headers();
    missing_guard.remove("x-hito-request");
    for (headers, expected) in [
        (anonymous, StatusCode::UNAUTHORIZED),
        (foreign_account, StatusCode::FORBIDDEN),
        (foreign_origin, StatusCode::FORBIDDEN),
        (missing_guard, StatusCode::FORBIDDEN),
        (peer.headers(), StatusCode::NOT_FOUND),
    ] {
        let (status, _, result) = fixture::exchange(
            &library.app,
            "POST",
            &library.lookup_path(),
            library.lookup_body(),
            headers,
        )
        .with_subscriber(subscriber.clone())
        .await;
        assert_eq!(status, expected);
        assert!(result.get("document").is_none());
        provider.assert_lookups(0).await;
    }
    let mut stale = library.lookup_body();
    stale["installation_revision"] = json!(library.installation_revision + 1);
    let (status, headers, result) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        stale,
        library.account.headers(),
    )
    .with_subscriber(subscriber.clone())
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(result.get("document").is_none());
    assert!(headers["cache-control"] == "private, no-store");
    provider.assert_lookups(0).await;
    assert!(library.snapshot().await == before);
    assert!(peer_snapshot(&library.app, &peer, &playlist).await == peer_before);

    let (status, headers, result) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(subscriber)
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["cache-control"] == "private, no-store");
    assert!(result["status"] == "ready" && result["document"] == json!(provider::response(true)));
    provider.assert_lookups(1).await;
    assert!(library.snapshot().await == before);
    assert!(peer_snapshot(&library.app, &peer, &playlist).await == peer_before);
    library.assert_original();
    let cookie = peer.headers()["cookie"].to_str().unwrap().to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["not_found", "conflict", "ready"],
        &[PEER_NAME, PEER_PASSWORD, &playlist, &cookie, token, &hash],
    );
}

async fn peer(library: &Library) -> (Account, String) {
    let (status, user) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({
                "username": "owned-lyric-peer", "password": PEER_PASSWORD,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(&library.app, "owned-lyric-peer", PEER_PASSWORD).await;
    assert!(json!(peer.id) == user["id"] && peer.id != library.account.id);
    let playlist = Uuid::new_v4();
    let operation = Operation {
        id: Uuid::new_v4(),
        action: Action::PlaylistCreate {
            playlist_id: playlist,
            name: PEER_NAME.into(),
        },
    };
    let (status, receipt) = peer
        .request(
            &library.app,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "owned-lyric-peer-client", "operations": [operation],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(receipt == json!({"acknowledged": [operation.id]}));
    let playlist = playlist.to_string();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "GET",
            &format!("/api/v1/playlists/{playlist}"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    (peer, playlist)
}

async fn peer_snapshot(app: &Router, peer: &Account, playlist: &str) -> Value {
    let detail = peer
        .get(app, &format!("/api/v1/playlists/{playlist}"))
        .await;
    assert!(detail["playlist"]["name"] == PEER_NAME && detail["entries"] == json!([]));
    json!({
        "catalog": peer.get(app, "/api/v1/recordings").await,
        "libraries": peer.get(app, "/api/v1/libraries").await,
        "playlists": peer.get(app, "/api/v1/playlists").await,
        "detail": detail,
        "events": peer.get(app, "/api/v1/sync?cursor=0").await,
        "activity": peer.get(app, "/api/v1/activity").await,
        "plugins": peer.get(app, "/api/v1/plugins").await,
    })
}
