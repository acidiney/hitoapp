#[path = "../fixtures/credited_artist_authority.rs"]
mod owned;

use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest, PlaylistDetailRequest, PlaylistItemsRequest};
use owned::{COVER, Library, Owned, PROVIDER_ARTIST, Response, Storage, TOKEN};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

struct Context {
    owner: String,
    peer: String,
    recording: String,
    artist: String,
    copies: Vec<String>,
    installation: Value,
}

#[tokio::test]
async fn clearing_credentials_discards_a_held_credited_artist_image() {
    let mut owned = Owned::new();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let context = hang_guard(
            "owned credited photo credential setup",
            Box::pin(async {
                owned.open().await;
                Box::pin(prepare(&owned.library)).await
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&mut owned, &context)),
        )
        .await
        .expect("The credited photo authority workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    owned.close().await;
    let (response, before_assets, before_bytes, storage) = match outcome {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert!(
        matches!(
            response.status,
            StatusCode::ACCEPTED | StatusCode::NOT_FOUND | StatusCode::CONFLICT
        ) && storage.assets == before_assets
            && storage.bytes == before_bytes,
        "A photo authorized before credential clear must never become Ready or add retained bytes: status={}, before_assets={}, retained_assets={}, before_bytes={}, retained_bytes={}",
        response.status,
        before_assets,
        storage.assets,
        before_bytes,
        storage.bytes
    );
}

async fn prepare(library: &Library) -> Context {
    let setup = library
        .exchange(
            None,
            "POST",
            "/api/v1/setup",
            json!({
                "username": "owner", "password": "owned credited image owner password"
            }),
        )
        .await;
    assert_eq!(setup.status, StatusCode::CREATED);
    let owner = setup.json()["token"].as_str().unwrap().to_owned();
    let offered = library
        .exchange(
            Some(&owner),
            "POST",
            "/api/v1/plugins/services",
            json!({
                "name": "Owned credited image", "base_url": library.provider().url
            }),
        )
        .await;
    assert_eq!(offered.status, StatusCode::CREATED);
    let offered = offered.json();
    library.provider().require_bearer(TOKEN);
    let installed = library.exchange(Some(&owner), "POST", "/api/v1/plugins", json!({
        "service_id": offered["id"], "service_revision": offered["revision"], "bearer_token": TOKEN
    })).await;
    assert_eq!(installed.status, StatusCode::CREATED);
    let installation = installed.json();
    assert_eq!(installation["credential_set"], true);
    assert!(!installation.to_string().contains(TOKEN));
    let mut copies = Vec::new();
    for _ in 0..2 {
        let imported = library.exchange(Some(&owner), "POST",
            &format!("/api/v1/plugins/{}/playlists/import", installation["id"].as_str().unwrap()),
            json!({"request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
                "playlist_reference": "https://provider.invalid/owned-storage"})).await;
        assert_eq!(imported.status, StatusCode::OK);
        copies.push(imported.json()["playlist_id"].as_str().unwrap().to_owned());
    }
    assert_ne!(copies[0], copies[1]);
    let requests = library
        .provider()
        .requests()
        .into_iter()
        .filter(|request| {
            matches!(
                request.operation.as_str(),
                "v1/playlists/detail" | "v1/playlists/items"
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        (
            "v1/playlists/detail",
            serde_json::to_value(PlaylistDetailRequest::Reference {
                reference: "https://provider.invalid/owned-storage".into(),
            })
            .unwrap(),
        ),
        (
            "v1/playlists/items",
            serde_json::to_value(PlaylistItemsRequest {
                playlist_id: "owned:storage/playlist".into(),
                snapshot: "owned-storage:1".into(),
                cursor: None,
                limit: 100,
            })
            .unwrap(),
        ),
        (
            "v1/playlists/detail",
            serde_json::to_value(PlaylistDetailRequest::Canonical {
                playlist_id: "owned:storage/playlist".into(),
            })
            .unwrap(),
        ),
    ];
    assert_eq!(requests.len(), 2 * expected.len());
    for import in requests.chunks_exact(expected.len()) {
        for (request, (operation, body)) in import.iter().zip(&expected) {
            assert_eq!(request.operation, *operation);
            assert_eq!(&request.json(), body);
            assert_eq!(request.protocol().as_deref(), Some("1"));
            assert!(request.query.is_none() && request.has_authorization());
        }
    }
    let recordings = library.get(&owner, "/api/v1/recordings").await;
    assert_eq!(recordings["total"], 1);
    let recording = recordings["items"][0]["id"].as_str().unwrap().to_owned();
    let artists = library.get(&owner, "/api/v1/artists").await;
    assert_eq!(artists["total"], 1);
    assert_eq!(artists["items"][0]["name"], "Owned storage performer");
    assert_eq!(artists["items"][0]["provider_photo"], true);
    let artist = artists["items"][0]["id"].as_str().unwrap().to_owned();
    let operations = [
        json!({"kind": "recording_title_set", "recording_id": recording, "title": "My retained photo title"}),
        json!({"kind": "like_set", "recording_id": recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": recording, "position_ms": 4200, "speed": 1.25}),
    ].into_iter().map(|action| json!({"id": Uuid::new_v4(), "action": action})).collect::<Vec<_>>();
    let saved = library
        .exchange(
            Some(&owner),
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "owned-credited-photo", "operations": operations
            }),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK);
    assert_eq!(
        saved.json()["acknowledged"],
        json!(
            operations
                .iter()
                .map(|operation| operation["id"].clone())
                .collect::<Vec<_>>()
        )
    );
    let credentials = json!({"username": "peer", "password": "owned credited image peer password"});
    let created = library
        .exchange(Some(&owner), "POST", "/api/v1/users", credentials.clone())
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let logged = library
        .exchange(None, "POST", "/api/v1/login", credentials)
        .await;
    assert_eq!(logged.status, StatusCode::OK);
    let peer = logged.json()["token"].as_str().unwrap().to_owned();
    Context {
        owner,
        peer,
        recording,
        artist,
        copies,
        installation,
    }
}

async fn snapshot(library: &Library, context: &Context) -> Value {
    let mut copies = Vec::new();
    for playlist in &context.copies {
        let copy = library
            .get(&context.owner, &format!("/api/v1/playlists/{playlist}"))
            .await;
        let entries = copy["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_ne!(entries[0]["entry_id"], entries[1]["entry_id"]);
        assert!(
            entries
                .iter()
                .all(|entry| entry["recording_id"] == context.recording)
        );
        assert!(copy["origin"].is_object());
        copies.push(copy);
    }
    let progress = library
        .exchange(
            Some(&context.owner),
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [context.recording]}),
        )
        .await;
    assert_eq!(progress.status, StatusCode::OK);
    let peer_recordings = library.get(&context.peer, "/api/v1/recordings").await;
    assert_eq!(peer_recordings["total"], 0);
    let peer_plugins = library.get(&context.peer, "/api/v1/plugins").await;
    assert_eq!(peer_plugins["installations"], json!([]));
    json!({"copies": copies, "progress": progress.json(),
        "recordings": library.get(&context.owner, "/api/v1/recordings").await,
        "artists": library.get(&context.owner, "/api/v1/artists").await,
        "metadata": library.get(&context.owner, &format!("/api/v1/recordings/{}/metadata", context.recording)).await,
        "sources": library.get(&context.owner, &format!("/api/v1/recordings/{}/sources", context.recording)).await,
        "events": library.get(&context.owner, "/api/v1/sync?cursor=0").await,
        "peer_recordings": peer_recordings, "peer_plugins": peer_plugins,
        "peer_artists": library.get(&context.peer, "/api/v1/artists").await,
        "peer_playlists": library.get(&context.peer, "/api/v1/playlists").await,
        "peer_events": library.get(&context.peer, "/api/v1/sync?cursor=0").await})
}

async fn exercise(owned: &mut Owned, context: &Context) -> (Response, i64, i64, Storage) {
    let before = snapshot(&owned.library, context).await;
    owned.library.assert_originals();
    let before_storage = owned.library.storage().await;
    assert_eq!((before_storage.assets, before_storage.bytes), (0, 0));
    assert_eq!(
        (
            before_storage.pending,
            before_storage.reserved,
            before_storage.current
        ),
        (0, 0, 0)
    );
    assert!(before_storage.account_counts.is_empty());
    let image_path = format!("/api/v1/cover-art/credited-artist/{}", context.artist);
    let credential_path = format!(
        "/api/v1/plugins/{}/credential",
        context.installation["id"].as_str().unwrap()
    );
    let dispatches = owned.library.provider().requests().len();
    for (method, path, body) in [
        ("GET", image_path.clone(), Value::Null),
        (
            "GET",
            format!("/api/v1/playlists/{}", context.copies[0]),
            Value::Null,
        ),
        (
            "GET",
            format!("/api/v1/recordings/{}/metadata", context.recording),
            Value::Null,
        ),
        (
            "DELETE",
            credential_path.clone(),
            json!({"revision": context.installation["revision"]}),
        ),
    ] {
        let denied = owned
            .library
            .exchange(Some(&context.peer), method, &path, body)
            .await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
    }
    assert_eq!(owned.library.provider().requests().len(), dispatches);
    let queued = owned
        .library
        .exchange(Some(&context.owner), "GET", &image_path, Value::Null)
        .await;
    assert_eq!(queued.status, StatusCode::ACCEPTED);
    owned.start();
    assert!(
        owned.entered().await,
        "The owned old-credential image request must be held before public credential clear"
    );
    let image_requests = owned.library.provider().requests_for("v1/artwork");
    assert_eq!(image_requests.len(), 1);
    assert_eq!(
        image_requests[0].json(),
        serde_json::to_value(ArtworkRequest {
            entity: ArtworkEntity::Artist,
            provider_id: PROVIDER_ARTIST.into()
        })
        .unwrap()
    );
    let cleared = owned
        .library
        .exchange(
            Some(&context.owner),
            "DELETE",
            &credential_path,
            json!({"revision": context.installation["revision"]}),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK);
    let mut expected = context.installation.clone();
    expected["revision"] = json!(context.installation["revision"].as_i64().unwrap() + 1);
    expected["credential_set"] = json!(false);
    assert_eq!(cleared.json(), expected);
    assert!(!cleared.json().to_string().contains(TOKEN));
    let artists = owned.library.get(&context.owner, "/api/v1/artists").await;
    assert_eq!(artists["items"][0]["id"], context.artist);
    assert_eq!(artists["items"][0]["provider_photo"], true);
    assert!(
        owned.complete().await,
        "The released old-credential image worker and held response must join"
    );
    let response = owned
        .library
        .exchange(Some(&context.owner), "GET", &image_path, Value::Null)
        .await;
    assert_eq!(response.headers["cache-control"], "private, no-store");
    if response.status == StatusCode::OK {
        assert_eq!(response.headers["content-type"], "image/png");
        assert_eq!(response.bytes, COVER);
    }
    let storage = owned.library.storage().await;
    assert_eq!(storage.broken_references, 0);
    assert_eq!(storage.foreign_key_failures, 0);
    assert_eq!(storage.pending, 0);
    assert_eq!(storage.reserved, 0);
    assert_eq!(storage.current, 0);
    assert!(storage.account_counts.is_empty());
    assert_eq!(snapshot(&owned.library, context).await, before);
    owned.library.assert_originals();
    let denied = owned
        .library
        .exchange(Some(&context.peer), "GET", &image_path, Value::Null)
        .await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);
    assert_eq!(owned.library.provider().requests_for("v1/artwork").len(), 1);
    assert!(
        owned
            .library
            .provider()
            .requests()
            .iter()
            .all(|request| matches!(
                request.operation.as_str(),
                "v1/manifest" | "v1/playlists/detail" | "v1/playlists/items" | "v1/artwork"
            ))
    );
    assert_eq!(owned.library.provider().unexpected(), 0);
    assert_eq!(owned.library.provider().unmatched(), 0);
    (
        response,
        before_storage.assets,
        before_storage.bytes,
        storage,
    )
}
