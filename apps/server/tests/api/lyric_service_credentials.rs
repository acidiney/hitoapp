use super::support;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    fs,
    panic::{AssertUnwindSafe, resume_unwind},
};
use uuid::Uuid;

use crate::automatic_lrclib_fixture as lrclib;
use crate::{account, lyric_identity_fixture as library};

#[tokio::test]
async fn a_plugin_service_credential_failure_is_distinct_from_listener_auth_and_account_access() {
    let upstream = lrclib::Upstream::start().await;
    let config = hito_plugin_lrclib::Config::new(lrclib::TOKEN)
        .unwrap()
        .with_endpoint(upstream.server.url.parse().unwrap())
        .unwrap();
    let shutdown = config.shutdown_handle();
    let plugin = lrclib::Server::start(hito_plugin_lrclib::router(config).unwrap()).await;
    let result = AssertUnwindSafe(exercise(&plugin.url, &upstream))
        .catch_unwind()
        .await;
    shutdown.cancel();
    plugin.finish().await;
    upstream.server.finish().await;
    let (status, headers, response, installation) = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "An authenticated listener must receive a recoverable plugin connection failure"
    );
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(response["code"], "plugin_credentials_required");
    assert_eq!(
        response["plugin_connection"],
        json!({
            "installation_id": installation["id"],
            "installation_revision": installation["revision"],
        })
    );
    let fields = response.as_object().unwrap();
    assert_eq!(fields.len(), 3);
    assert!(
        fields
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty() && text.len() <= 240)
    );
    let encoded = response.to_string();
    for private in [lrclib::TOKEN, lrclib::TITLE, lrclib::ARTIST] {
        assert!(!encoded.contains(private));
    }
}

async fn exercise(
    plugin_url: &str,
    upstream: &lrclib::Upstream,
) -> (StatusCode, HeaderMap, Value, Value) {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("owned-local-music");
    fs::create_dir(&music).unwrap();
    let original = music.join("owned.wav");
    let bytes = support::write_wav(&original);
    let file_before = fs::metadata(&original).unwrap();
    let services = support::services(directory.path(), &music).await;
    let app = hito_server::router(services.clone());
    let owner = account::Account::setup(&app).await;
    let (status, _) = owner
        .request(
            &app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({"revision":0,"enabled":false,"providers":[]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let libraries = owner.get(&app, "/api/v1/libraries").await;
    let library = libraries
        .as_array()
        .unwrap()
        .iter()
        .find(|library| library["personal"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = owner
        .request(
            &app,
            "POST",
            &format!("/api/v1/libraries/{library}/scan"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(services.storage.run_next_job().await.unwrap());
    let recordings = owner.get(&app, "/api/v1/recordings").await;
    assert_eq!(recordings["total"], 1);
    assert_eq!(recordings["items"][0]["available"], true);
    let recording = recordings["items"][0]["id"].as_str().unwrap();
    let playlist = Uuid::new_v4();
    let operations = json!([
        {"id":Uuid::new_v4(),"action":{"kind":"recording_title_set","recording_id":recording,"title":lrclib::TITLE}},
        {"id":Uuid::new_v4(),"action":{"kind":"recording_artist_set","recording_id":recording,"artist":lrclib::ARTIST}},
        {"id":Uuid::new_v4(),"action":{"kind":"like_set","recording_id":recording,"liked":true}},
        {"id":Uuid::new_v4(),"action":{"kind":"progress_set","recording_id":recording,"position_ms":700,"speed":1.25}},
        {"id":Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":playlist,"name":"Owned lyric credential playlist"}},
        {"id":Uuid::new_v4(),"action":{"kind":"entry_insert","playlist_id":playlist,"entry_id":Uuid::new_v4(),"recording_id":recording,"position":0}}
    ]);
    let (status, synchronized) = owner
        .request(
            &app,
            "POST",
            "/api/v1/sync",
            json!({"device_id":"owned-lyric-credential-client","operations":operations}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(synchronized["acknowledged"].as_array().unwrap().len(), 6);
    let installation = owner
        .connect(&app, plugin_url, "Owned LRCLIB credential service", None)
        .await;
    assert_eq!(installation["credential_set"], false);
    assert_eq!(installation["manifest"]["id"], "org.hito.lrclib");
    let (status, created_peer) = owner
        .request(
            &app,
            "POST",
            "/api/v1/users",
            json!({"username":"lyric-credential-peer","password":"owned lyric credential peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &app,
        "lyric-credential-peer",
        "owned lyric credential peer password",
    )
    .await;
    assert_eq!(created_peer["id"], peer.id);
    let (status, _) = owner
        .request(
            &app,
            "PUT",
            &format!("/api/v1/libraries/{library}/members/{}", peer.id),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let peer_recordings = peer.get(&app, "/api/v1/recordings").await;
    assert_eq!(peer_recordings["items"][0]["id"], recording);
    assert!(
        peer.get(&app, "/api/v1/plugins").await["installations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let before = snapshot(&app, &owner, recording, playlist).await;
    assert_eq!(before["identities"]["items"], json!([]));
    assert_eq!(before["liked"]["total"], 1);
    assert_eq!(before["playlist"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(before["progress"][0]["position_ms"], 700);
    let path = format!("/api/v1/recordings/{recording}/lyrics/candidates");
    let request = json!({
        "installation_id":installation["id"],
        "installation_revision":installation["revision"],
        "query":format!("{} {}", lrclib::ARTIST, lrclib::TITLE),
    });
    let (status, unauthenticated) = support::call(&app, "POST", &path, request.clone(), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(unauthenticated.get("plugin_connection").is_none());
    assert_ne!(unauthenticated["code"], "plugin_credentials_required");
    let (status, refused) = peer.request(&app, "POST", &path, request.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(refused.get("plugin_connection").is_none());
    assert_ne!(refused["code"], "plugin_credentials_required");
    upstream.assert_calls(&[]);
    let (status, headers, response) =
        account::exchange(&app, "POST", &path, request, owner.headers()).await;
    upstream.assert_calls(&[]);
    assert_eq!(snapshot(&app, &owner, recording, playlist).await, before);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    let file_after = fs::metadata(&original).unwrap();
    assert_eq!(file_after.len(), file_before.len());
    assert_eq!(
        file_after.modified().unwrap(),
        file_before.modified().unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(file_after.ino(), file_before.ino());
    }
    (status, headers, response, installation)
}

async fn snapshot(
    app: &Router,
    account: &account::Account,
    recording: &str,
    playlist: Uuid,
) -> Value {
    let base = format!("/api/v1/recordings/{recording}");
    let (status, progress) = account
        .request(
            app,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids":[recording]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    json!({
        "recordings":account.get(app, "/api/v1/recordings").await,
        "libraries":account.get(app, "/api/v1/libraries").await,
        "metadata":account.get(app, &format!("{base}/metadata")).await,
        "sources":account.get(app, &format!("{base}/sources")).await,
        "identities":account.get(app, &format!("{base}/lyrics/identities")).await,
        "liked":account.get(app, "/api/v1/recordings?liked=true").await,
        "playlists":account.get(app, "/api/v1/playlists").await,
        "playlist":account.get(app, &format!("/api/v1/playlists/{playlist}")).await,
        "activity":account.get(app, "/api/v1/activity").await,
        "events":account.get(app, "/api/v1/sync?cursor=0").await,
        "progress":progress,
    })
}

#[tokio::test]
async fn rejected_service_credentials_are_reported_as_private_health_without_affecting_a_peer() {
    let upstream = lrclib::Upstream::start().await;
    let config = hito_plugin_lrclib::Config::new(lrclib::TOKEN)
        .unwrap()
        .with_endpoint(upstream.server.url.parse().unwrap())
        .unwrap();
    let shutdown = config.shutdown_handle();
    let plugin = lrclib::Server::start(hito_plugin_lrclib::router(config).unwrap()).await;
    let result = AssertUnwindSafe(exercise_health(&plugin.url, &upstream))
        .catch_unwind()
        .await;
    shutdown.cancel();
    plugin.finish().await;
    upstream.server.finish().await;
    let installation = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        installation["health"]["state"], "credentials_required",
        "The owner's rejected service bearer must be visible in private plugin health"
    );
}

async fn exercise_health(plugin_url: &str, upstream: &lrclib::Upstream) -> Value {
    let library = library::Library::prepare().await;
    let installation = library
        .account
        .connect(
            &library.app,
            plugin_url,
            "Owned credential health service",
            None,
        )
        .await;
    let settings = library.account.get(&library.app, "/api/v1/plugins").await;
    let service = settings["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == installation["service_id"])
        .unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"lyric-health-peer","password":"owned lyric health peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = account::Account::login(
        &library.app,
        "lyric-health-peer",
        "owned lyric health peer password",
    )
    .await;
    let (status, peer_installation) = peer
        .request(
            &library.app,
            "POST",
            "/api/v1/plugins",
            json!({
                "service_id":service["id"],
                "service_revision":service["revision"],
                "bearer_token":lrclib::TOKEN,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(installation["id"], peer_installation["id"]);
    assert_eq!(installation["service_id"], peer_installation["service_id"]);
    let before = library.snapshot().await;
    let identities_path = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
    let identities = library.account.get(&library.app, &identities_path).await;
    let (status, unauthenticated) =
        support::call(&library.app, "GET", "/api/v1/plugins", Value::Null, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(unauthenticated.get("plugin_connection").is_none());
    let (status, owner_headers, owner_settings) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_headers["cache-control"], "private, no-store");
    let owner_installations = owner_settings["installations"].as_array().unwrap();
    assert_eq!(owner_installations.len(), 1);
    assert_eq!(owner_installations[0]["id"], installation["id"]);
    assert_eq!(owner_installations[0]["credential_set"], false);
    let (status, peer_headers, peer_settings) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        peer.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(peer_headers["cache-control"], "private, no-store");
    let peer_installations = peer_settings["installations"].as_array().unwrap();
    assert_eq!(peer_installations.len(), 1);
    assert_eq!(peer_installations[0]["id"], peer_installation["id"]);
    assert_eq!(peer_installations[0]["credential_set"], true);
    assert!(peer_installations[0].get("health").is_none());
    upstream.assert_calls(&[]);
    assert_eq!(library.snapshot().await, before);
    assert_eq!(
        library.account.get(&library.app, &identities_path).await,
        identities
    );
    library.assert_original();
    owner_installations[0].clone()
}
