use super::{
    AssertUnwindSafe, Duration, FutureExt, Library, StatusCode, assert_private_logs,
    fixture::{self, Account},
    json, logs, provider,
    providers::attached,
    resume_unwind,
};
use hito_core::{Action, Operation};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

const PEER_PLAYLIST: &str = "Owned private lyric choice peer playlist";

#[tokio::test]
async fn local_lyric_provider_choices_require_current_account_and_current_approved_connections() {
    let primary = provider::Provider::start().await;
    let secondary = attached::Provider::start().await;
    let sources_only = attached::Provider::without_lyrics().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(authority(&primary, &secondary, &sources_only)).catch_unwind(),
    )
    .await;
    sources_only.finish().await;
    secondary.finish().await;
    primary.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The owned lyric provider authority workflow must finish within its deadline")
        }
    }
}

async fn authority(
    primary: &provider::Provider,
    secondary: &attached::Provider,
    sources_only: &attached::Provider,
) {
    let library = Library::prepare(primary).await;
    let secondary_installation = attach(&library, secondary).await;
    let sources_installation = attach(&library, sources_only).await;
    let (status, _) = library.account.request(&library.app, "POST", "/api/v1/users",
        json!({"username":"owned-lyric-choice-peer", "password":"owned lyric choice peer password"})).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        &library.app,
        "owned-lyric-choice-peer",
        "owned lyric choice peer password",
    )
    .await;
    let operation = Operation {
        id: Uuid::new_v4(),
        action: Action::PlaylistCreate {
            playlist_id: Uuid::new_v4(),
            name: PEER_PLAYLIST.into(),
        },
    };
    let (status, receipt) = peer
        .request(
            &library.app,
            "POST",
            "/api/v1/sync",
            json!({"device_id": "owned-lyric-choice-peer-client", "operations": [operation]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(receipt == json!({"acknowledged": [operation.id]}));
    let peer_before = peer_snapshot(&library, &peer).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let dispatch = tracing::Dispatch::new(capture.subscriber());
    let path = format!("{}/providers", library.lookup_path());
    let mut anonymous = library.account.headers();
    anonymous.remove("cookie");
    let mut wrong_account = library.account.headers();
    wrong_account.insert("x-hito-account", peer.id.parse().unwrap());
    for (headers, expected) in [
        (anonymous, StatusCode::UNAUTHORIZED),
        (wrong_account, StatusCode::FORBIDDEN),
        (peer.headers(), StatusCode::NOT_FOUND),
    ] {
        let (status, _, denied) =
            fixture::exchange(&library.app, "GET", &path, Value::Null, headers)
                .with_subscriber(dispatch.clone())
                .await;
        assert_eq!(status, expected);
        assert!(denied.get("items").is_none());
    }
    let (status, _, denied) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/recordings/not-a-recording-uuid/lyrics/providers",
        Value::Null,
        library.account.headers(),
    )
    .with_subscriber(dispatch.clone())
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(denied.get("items").is_none());
    let list = read(&library, &dispatch).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    assert!(list["items"][0]["observation_id"] == library.observation);
    assert!(list["items"][1]["installation_id"] == secondary_installation["id"]);
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["installation_id"] != sources_installation["id"])
    );
    assert!(library.snapshot().await == before);
    assert!(peer_snapshot(&library, &peer).await == peer_before);
    let primary_id = before["metadata"]["selected_observation"]["plugin_installation_id"]
        .as_str()
        .unwrap();
    let (status, changed) = library
        .account
        .request(
            &library.app,
            "DELETE",
            &format!("/api/v1/plugins/{primary_id}/credential"),
            json!({"revision": library.installation_revision}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(changed["revision"].as_i64().unwrap() == library.installation_revision + 1);
    let before = library.snapshot().await;
    let list = read(&library, &dispatch).await;
    assert!(list["items"][0]["installation_revision"] == changed["revision"]);
    assert!(library.snapshot().await == before);
    let (status, _, denied) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(dispatch.clone())
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(denied.get("document").is_none());
    assert!(library.snapshot().await == before);
    let service = before["plugins"]["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == secondary_installation["service_id"])
        .unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "DELETE",
            &format!(
                "/api/v1/plugins/services/{}",
                service["id"].as_str().unwrap()
            ),
            json!({"revision": service["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let before = library.snapshot().await;
    let list = read(&library, &dispatch).await;
    assert!(
        list["items"].as_array().unwrap().len() == 1
            && list["items"][0]["installation_id"] == primary_id
    );
    assert!(library.snapshot().await == before);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "DELETE",
            &format!("/api/v1/plugins/{primary_id}"),
            json!({"revision": changed["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let before = library.snapshot().await;
    let list = read(&library, &dispatch).await;
    assert!(list == json!({"recording_id": library.recording, "items": []}));
    assert!(library.snapshot().await == before);
    assert!(peer_snapshot(&library, &peer).await == peer_before);
    primary.assert_lookups(0).await;
    secondary.assert_calls(2, 2, 0);
    sources_only.assert_calls(2, 2, 0);
    library.assert_original();
    capture.assert_provider_lists(&["not_found", "invalid", "read", "read", "read", "read"]);
    capture.assert_core_outcomes(&["conflict"]);
    let cookie = peer.headers()["cookie"].to_str().unwrap().to_owned();
    let token = cookie.strip_prefix("hito_session=").unwrap();
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    assert_private_logs(
        &capture,
        &library,
        primary,
        &["conflict"],
        &[
            attached::RECORDING,
            attached::TOKEN,
            attached::TITLE,
            &secondary.url,
            &sources_only.url,
            PEER_PLAYLIST,
            &cookie,
            token,
            &hash,
        ],
    );
}

async fn attach(library: &Library, provider: &attached::Provider) -> Value {
    let installation = library
        .account
        .connect(
            &library.app,
            &provider.url,
            attached::NAME,
            Some(attached::TOKEN),
        )
        .await;
    let candidate = json!({"installation_id": installation["id"],
        "installation_revision": installation["revision"], "provider_recording_id": attached::RECORDING});
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("/api/v1/recordings/{}/sources/review", library.recording),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, result) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("/api/v1/recordings/{}/sources/attach", library.recording),
            json!({"candidate": candidate, "review_stamp": review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(result["id"] == library.recording);
    provider.assert_calls(2, 2, 0);
    installation
}

async fn read(library: &Library, dispatch: &tracing::Dispatch) -> Value {
    let (status, headers, value) = fixture::exchange(
        &library.app,
        "GET",
        &format!("{}/providers", library.lookup_path()),
        Value::Null,
        library.account.headers(),
    )
    .with_subscriber(dispatch.clone())
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["cache-control"] == "private, no-store");
    value
}

async fn peer_snapshot(library: &Library, peer: &Account) -> Value {
    let mut values = Vec::new();
    for path in [
        "/api/v1/recordings",
        "/api/v1/playlists",
        "/api/v1/activity",
        "/api/v1/sync?cursor=0",
    ] {
        values.push(peer.get(&library.app, path).await);
    }
    let plugins = peer.get(&library.app, "/api/v1/plugins").await;
    values.push(json!({"installations": plugins["installations"],
        "credential_storage_ready": plugins["credential_storage_ready"]}));
    json!(values)
}
