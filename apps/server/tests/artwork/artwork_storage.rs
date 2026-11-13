#[path = "../fixtures/artwork_storage.rs"]
mod fixture;

use axum::http::{Method, StatusCode};
use fixture::{ARTIST, COVER, Library, Storage};
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use hito_plugin_sdk::{PlaylistDetailRequest, PlaylistItemsRequest};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

const GLOBAL_BYTES: i64 = 256 * 1024 * 1024;
const ACCOUNT_RESERVATIONS: i64 = 16 * 2 * 1024 * 1024;

struct Account {
    token: String,
    recording: String,
    playlist: String,
    jobs: Vec<Value>,
}

struct Context {
    accounts: Vec<Account>,
    requester: String,
}

#[tokio::test]
async fn automatic_artwork_cannot_consume_the_global_explicit_reservation() {
    let mut library = Library::retain();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let context = hang_guard(
            "owned global artwork reservation setup",
            Box::pin(async {
                library.open().await;
                Box::pin(prepare(&library)).await
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&mut library, &context)),
        )
        .await
        .expect("The owned global artwork workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    library.close().await;
    let (response, storage) = match outcome {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert!(
        response.status == StatusCode::CONFLICT
            && response.json()["error"]
                .as_str()
                .is_some_and(|error| error.to_lowercase().contains("storage"))
            && storage.assets == 0
            && storage.bytes + storage.reserved <= GLOBAL_BYTES,
        "Automatic artwork must report local storage capacity and preserve the global byte promise: status={}, assets={}, retained_bytes={}, reserved_bytes={}",
        response.status,
        storage.assets,
        storage.bytes,
        storage.reserved
    );
}

async fn prepare(library: &Library) -> Context {
    let setup = library
        .exchange(
            None,
            "POST",
            "/api/v1/setup",
            json!({
                "username": "owner", "password": "owned global artwork owner password"
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
                "name": "Owned global artwork", "base_url": library.provider().url
            }),
        )
        .await;
    assert_eq!(offered.status, StatusCode::CREATED);
    let offered = offered.json();
    let mut tokens = vec![owner.clone()];
    for index in 1..=8 {
        let credentials = json!({"username": format!("member{index}"),
            "password": "owned global artwork member password"});
        let created = library
            .exchange(Some(&owner), "POST", "/api/v1/users", credentials.clone())
            .await;
        assert_eq!(created.status, StatusCode::CREATED);
        let logged = library
            .exchange(None, "POST", "/api/v1/login", credentials)
            .await;
        assert_eq!(logged.status, StatusCode::OK);
        tokens.push(logged.json()["token"].as_str().unwrap().to_owned());
    }
    let requester = tokens.pop().unwrap();
    let mut accounts = Vec::new();
    for (index, token) in tokens.into_iter().enumerate() {
        let installed = library
            .exchange(
                Some(&token),
                "POST",
                "/api/v1/plugins",
                json!({
                    "service_id": offered["id"], "service_revision": offered["revision"]
                }),
            )
            .await;
        assert_eq!(installed.status, StatusCode::CREATED);
        let installed = installed.json();
        let mut jobs = Vec::new();
        let mut playlists = std::collections::HashSet::new();
        for _ in 0..16 {
            let imported = library.exchange(Some(&token), "POST",
                &format!("/api/v1/plugins/{}/playlists/import", installed["id"].as_str().unwrap()),
                json!({"request_id": Uuid::new_v4(), "installation_revision": installed["revision"],
                    "playlist_reference": "https://provider.invalid/owned-storage"})).await;
            assert_eq!(imported.status, StatusCode::OK);
            let playlist = imported.json()["playlist_id"].as_str().unwrap().to_owned();
            assert!(playlists.insert(playlist.clone()));
            let queued = library
                .exchange(
                    Some(&token),
                    "POST",
                    &format!("/api/v1/playlists/{playlist}/artwork/fetch"),
                    Value::Null,
                )
                .await;
            assert_eq!(queued.status, StatusCode::ACCEPTED);
            let queued = queued.json();
            assert_eq!(queued["playlist_id"], playlist);
            assert_eq!(queued["status"], "queued");
            assert_eq!(queued["attempts"], 0);
            jobs.push(queued);
        }
        assert_eq!(playlists.len(), 16);
        let recordings = library.get(&token, "/api/v1/recordings").await;
        assert_eq!(recordings["total"], 1);
        let recording = recordings["items"][0]["id"].as_str().unwrap().to_owned();
        let operations = [
            json!({"kind": "recording_title_set", "recording_id": recording,
                "title": format!("My owned storage title {index}")}),
            json!({"kind": "like_set", "recording_id": recording, "liked": true}),
            json!({"kind": "progress_set", "recording_id": recording,
                "position_ms": 4200, "speed": 1.25}),
        ]
        .into_iter()
        .map(|action| json!({"id": Uuid::new_v4(), "action": action}))
        .collect::<Vec<_>>();
        let saved = library
            .exchange(
                Some(&token),
                "POST",
                "/api/v1/sync",
                json!({
                    "device_id": format!("owned-storage-{index}"), "operations": operations
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
        accounts.push(Account {
            token,
            recording,
            playlist: jobs[0]["playlist_id"].as_str().unwrap().into(),
            jobs,
        });
    }
    assert_eq!(accounts.len(), 8);
    assert_import_protocol(library);
    assert!(library.provider().requests_for("v1/artwork").is_empty());
    Context {
        accounts,
        requester,
    }
}

fn assert_import_protocol(library: &Library) {
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
    assert_eq!(requests.len(), 128 * expected.len());
    for import in requests.chunks_exact(expected.len()) {
        for (request, (operation, body)) in import.iter().zip(&expected) {
            assert_eq!(request.method, Method::POST);
            assert_eq!(request.operation, *operation);
            assert_eq!(request.protocol().as_deref(), Some("1"));
            assert!(request.query.is_none() && !request.has_authorization());
            assert_eq!(&request.json(), body);
        }
    }
}

async fn snapshot(library: &Library, context: &Context) -> Value {
    let mut accounts = Vec::new();
    for account in &context.accounts {
        let progress = library
            .exchange(
                Some(&account.token),
                "POST",
                "/api/v1/recordings/progress",
                json!({"recording_ids": [account.recording]}),
            )
            .await;
        assert_eq!(progress.status, StatusCode::OK);
        accounts.push(json!({
            "recordings": library.get(&account.token, "/api/v1/recordings").await,
            "playlists": library.get(&account.token, "/api/v1/playlists").await,
            "playlist": library.get(&account.token, &format!("/api/v1/playlists/{}", account.playlist)).await,
            "metadata": library.get(&account.token, &format!("/api/v1/recordings/{}/metadata", account.recording)).await,
            "sources": library.get(&account.token, &format!("/api/v1/recordings/{}/sources", account.recording)).await,
            "events": library.get(&account.token, "/api/v1/sync?cursor=0").await,
            "progress": progress.json(),
        }));
    }
    let peer_recordings = library.get(&context.requester, "/api/v1/recordings").await;
    assert_eq!(peer_recordings["total"], 0);
    json!({"accounts": accounts, "peer_recordings": peer_recordings,
        "peer_playlists": library.get(&context.requester, "/api/v1/playlists").await,
        "peer_plugins": library.get(&context.requester, "/api/v1/plugins").await,
        "peer_events": library.get(&context.requester, "/api/v1/sync?cursor=0").await})
}

fn reservations(storage: &Storage) {
    assert_eq!(storage.pending, 128);
    assert_eq!(storage.reserved, GLOBAL_BYTES);
    assert_eq!(storage.current, 128);
    assert_eq!(storage.account_counts, vec![(16, ACCOUNT_RESERVATIONS); 8]);
    assert_eq!(storage.broken_references, 0);
    assert_eq!(storage.foreign_key_failures, 0);
}

async fn exercise(library: &mut Library, context: &Context) -> (fixture::Response, Storage) {
    let before_storage = library.storage().await;
    reservations(&before_storage);
    assert_eq!((before_storage.assets, before_storage.bytes), (0, 0));
    let before = snapshot(library, context).await;
    library.assert_originals();
    let peer = &context.requester;
    let owner = &context.accounts[0];
    let dispatches = library.provider().requests().len();
    for path in [
        format!("/api/v1/playlists/{}", owner.playlist),
        format!(
            "/api/v1/playlist-artwork-fetches/{}",
            owner.jobs[0]["id"].as_str().unwrap()
        ),
        format!("/api/v1/playlists/{}/artwork", owner.playlist),
    ] {
        let denied = library
            .exchange(Some(peer), "GET", &path, Value::Null)
            .await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
    }
    assert_eq!(library.provider().requests().len(), dispatches);
    let image_path = format!("/api/v1/cover-art/artist/{ARTIST}");
    let queued = library
        .exchange(Some(peer), "GET", &image_path, Value::Null)
        .await;
    assert!(matches!(
        queued.status,
        StatusCode::ACCEPTED | StatusCode::CONFLICT
    ));
    library
        .hito()
        .catalog
        .run_next_cover_art_lookup_until(std::future::pending::<()>())
        .await
        .unwrap();
    let response = library
        .exchange(Some(peer), "GET", &image_path, Value::Null)
        .await;
    assert_eq!(response.headers["cache-control"], "private, no-store");
    if response.status == StatusCode::OK {
        assert_eq!(response.headers["content-type"], "image/png");
        assert_eq!(response.bytes, COVER);
    }
    let storage = library.storage().await;
    reservations(&storage);
    for account in &context.accounts {
        for job in &account.jobs {
            let receipt = library
                .get(
                    &account.token,
                    &format!(
                        "/api/v1/playlist-artwork-fetches/{}",
                        job["id"].as_str().unwrap()
                    ),
                )
                .await;
            assert_eq!(&receipt, job);
        }
    }
    assert_eq!(snapshot(library, context).await, before);
    library.assert_originals();
    let requests = library.provider().requests();
    assert!(requests.iter().all(|request| matches!(
        request.operation.as_str(),
        "v1/manifest"
            | "v1/playlists/detail"
            | "v1/playlists/items"
            | "wiki/Special:EntityData/Q42.json"
            | "wiki/Special:FilePath/Owned.png"
    ) || request.operation == format!("artist/{ARTIST}")));
    let metadata_requests = requests
        .iter()
        .filter(|request| !request.operation.starts_with("v1/"))
        .count();
    assert!(metadata_requests == 0 || metadata_requests == 3);
    assert_eq!(library.provider().unexpected(), 0);
    assert_eq!(library.provider().unmatched(), 0);
    (response, storage)
}
