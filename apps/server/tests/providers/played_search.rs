#[path = "../fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "../fixtures/artwork_http.rs"]
mod http;
use hito_core::test_support::content_service as provider;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};

async fn sign_in(address: SocketAddr, username: &str, password: &str) -> String {
    let session = http::call(
        address,
        "POST",
        "/api/v1/login",
        Some(json!({"username": username, "password": password})),
        None,
    )
    .await;
    assert_eq!(session.status, StatusCode::OK);
    session.json()["token"].as_str().unwrap().to_owned()
}

async fn found(address: SocketAddr, token: &str, q: &str) -> Value {
    let q = q.replace(' ', "%20");
    let names = |page: Value, field: &str| -> Value {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item[field].clone())
            .collect()
    };
    json!({
        "tracks": names(http::get(address, token, &format!("/api/v1/recordings?q={q}")).await, "title"),
        "albums": names(http::get(address, token, &format!("/api/v1/releases?q={q}")).await, "title"),
        "artists": names(http::get(address, token, &format!("/api/v1/artists?q={q}")).await, "name"),
    })
}

#[tokio::test]
async fn an_album_played_from_a_service_is_found_over_http_by_its_listener_only() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let services = host::open(directory.path(), "played-search-http").await;
    let mut server = host::Host::start(services).await;
    let provider = provider::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(async {
            provider
                .reply(
                    "v1/manifest",
                    json!({
                        "id": "org.hito.played-fixture", "name": "Played fixture",
                        "protocol_version": 1, "capabilities": ["search", "sources", "metadata"]
                    }),
                )
                .await;
            let setup = http::call(
                server.address,
                "POST",
                "/api/v1/setup",
                Some(json!({"username": "owner", "password": "played search HTTP password"})),
                None,
            )
            .await;
            assert_eq!(setup.status, StatusCode::CREATED);
            let token = setup.json()["token"].as_str().unwrap().to_owned();
            let offered = http::call(
                server.address,
                "POST",
                "/api/v1/plugins/services",
                Some(json!({"name": "Played fixture", "base_url": provider.url})),
                Some(&token),
            )
            .await;
            assert_eq!(offered.status, StatusCode::CREATED);
            let service = offered.json();
            let installed = http::call(
                server.address,
                "POST",
                "/api/v1/plugins",
                Some(json!({
                    "service_id": service["id"], "service_revision": service["revision"],
                    "bearer_token": null, "accept_manifest_digest": null
                })),
                Some(&token),
            )
            .await;
            assert_eq!(installed.status, StatusCode::CREATED);
            let installation = installed.json();
            for (id, title) in [("opening", "Quiet Harbour"), ("closing", "Last Lantern")] {
                let detail = json!({
                    "id": id, "title": title, "artist": "Lumen Tide", "duration_ms": 1000,
                    "artists": [{"name": "Lumen Tide", "role": "main"}]
                });
                let source = format!("{id}/audio");
                provider
                    .recording_reply("v1/recordings/detail", id, detail.clone())
                    .await;
                provider
                    .recording_reply(
                        "v1/sources",
                        id,
                        json!({"items": [{"id": source, "label": "Owned audio", "ready": true}]}),
                    )
                    .await;
                provider
                    .recording_reply(
                        "v1/recordings/metadata",
                        id,
                        json!({"recording": detail, "musical_title": title,
                            "album_title": "Northern Lanterns", "source_ids": [source]}),
                    )
                    .await;
                let saved = http::call(
                    server.address,
                    "POST",
                    &format!(
                        "/api/v1/plugins/{}/recordings",
                        installation["id"].as_str().unwrap()
                    ),
                    Some(json!({
                        "installation_revision": installation["revision"],
                        "provider_recording_id": id
                    })),
                    Some(&token),
                )
                .await;
                assert_eq!(saved.status, StatusCode::OK);
                let played = http::call(
                    server.address,
                    "POST",
                    "/api/v1/sync",
                    Some(json!({"device_id": "played-search", "operations": [{
                        "id": uuid::Uuid::new_v4(),
                        "action": {"kind": "progress_set", "recording_id": saved.json()["id"],
                            "position_ms": 1000, "speed": 1.0}
                    }]})),
                    Some(&token),
                )
                .await;
                assert_eq!(played.status, StatusCode::OK);
            }
            assert_eq!(
                found(server.address, &token, "quiet harb").await["tracks"],
                json!(["Quiet Harbour"])
            );
            assert_eq!(
                found(server.address, &token, "northern lanterns").await["albums"],
                json!(["Northern Lanterns"])
            );
            let by_artist = found(server.address, &token, "lumen").await;
            assert_eq!(by_artist["albums"], json!(["Northern Lanterns"]));
            assert_eq!(by_artist["artists"], json!(["Lumen Tide"]));
            assert_eq!(by_artist["tracks"].as_array().unwrap().len(), 2);

            let member = http::call(
                server.address,
                "POST",
                "/api/v1/users",
                Some(json!({"username": "member", "password": "played search member password"})),
                Some(&token),
            )
            .await;
            assert_eq!(member.status, StatusCode::CREATED);
            let libraries = http::get(server.address, &token, "/api/v1/libraries").await;
            let shared = libraries
                .as_array()
                .unwrap()
                .iter()
                .find(|library| library["personal"] == false)
                .unwrap();
            let granted = http::call(
                server.address,
                "PUT",
                &format!(
                    "/api/v1/libraries/{}/members/{}",
                    shared["id"].as_str().unwrap(),
                    member.json()["id"].as_str().unwrap()
                ),
                None,
                Some(&token),
            )
            .await;
            assert_eq!(granted.status, StatusCode::NO_CONTENT);
            let member = sign_in(server.address, "member", "played search member password").await;
            for q in ["quiet harbour", "northern lanterns", "lumen tide"] {
                assert_eq!(
                    found(server.address, &member, q).await,
                    json!({"tracks": [], "albums": [], "artists": []})
                );
            }
        })
        .catch_unwind(),
    )
    .await;
    server.request_shutdown();
    let joined = server.join().await;
    provider.shutdown().await;
    assert!(joined, "the owned HTTP host must stop and join");
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the played search workflow must settle within its deadline"),
    }
}
