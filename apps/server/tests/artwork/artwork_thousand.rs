use crate::{
    artwork_thousand_corpus::{self as corpus, COUNT, Corpus},
    artwork_thousand_http::{Library, Storage},
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    panic::{AssertUnwindSafe, resume_unwind},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[tokio::test]
async fn one_thousand_distinct_album_covers_survive_beside_protected_artwork() {
    let mut library = Library::retain();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let started = Instant::now();
        let images = Corpus::create(&library.corpus_directory()).await;
        corpus::report("source", &images.source_sizes.iter().copied().map(Some).collect::<Vec<_>>());
        eprintln!("artwork_thousand stage=corpus distinct_sources={} distinct_source_pixels={} elapsed_ms={}", images.source_sizes.len(), images.source_pixels.len(), started.elapsed().as_millis());
        hang_guard("owned thousand-cover host setup", Box::pin(library.open(&images))).await;
        let setup = hang_guard("owned thousand-cover account and sentinel setup", Box::pin(prepare(&library))).await;
        tokio::time::timeout(Duration::from_secs(1800), Box::pin(exercise(&library, &images, &setup))).await
            .expect("The new finite thousand-cover acceptance workload must finish within its fixed 1800-second scale budget")
    })).catch_unwind().await;
    let cleanup = Instant::now();
    library.close().await;
    eprintln!(
        "artwork_thousand stage=cleanup retired=1 elapsed_ms={}",
        cleanup.elapsed().as_millis()
    );
    match outcome {
        Ok(ready) => assert_eq!(
            ready, COUNT,
            "Every frozen album must have its distinct retained cover after preservation checks and owned cleanup"
        ),
        Err(payload) => resume_unwind(payload),
    }
}

struct Setup {
    owner: String,
    peer: String,
    account: String,
    installed: Value,
    owner_sentinel: String,
    peer_sentinel: String,
    sentinel_recording: String,
    owner_sentinel_playlist: String,
    peer_sentinel_playlist: String,
    sentinels: Vec<(String, String, Vec<u8>)>,
    storage: Storage,
}

async fn prepare(library: &Library) -> Setup {
    let created = library
        .exchange(
            None,
            "POST",
            "/api/v1/setup",
            json!({"username":"owner", "password":"owned thousand-cover owner password"}),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let owner = created.json()["token"].as_str().unwrap().to_owned();
    let account = library.account_id(&owner).await;
    let offered = library
        .exchange(
            Some(&owner),
            "POST",
            "/api/v1/plugins/services",
            json!({"name":"Owned capacity provider", "base_url":library.provider().url}),
        )
        .await;
    assert_eq!(offered.status, StatusCode::CREATED);
    let offered = offered.json();
    let installed = connect(library, &owner, &offered).await;
    disable_automatic(library, &owner).await;
    let initial = library.inspect(&account).await;
    assert_eq!(
        (
            initial.account_assets,
            initial.account_bytes,
            initial.global_assets,
            initial.global_bytes,
            initial.pending,
            initial.reserved
        ),
        (0, 0, 0, 0, 0, 0)
    );
    initial.report("empty");
    let owner_sentinel_playlist = import(library, &owner, &installed, "sentinel").await;
    let catalog = records(library, &owner).await;
    assert_eq!(catalog.len(), 1);
    let sentinel_recording = catalog[0]["id"].as_str().unwrap().to_owned();
    let owner_sentinel = catalog[0]["release_id"].as_str().unwrap().to_owned();
    let mut sentinels = Vec::new();
    for path in [
        format!("/api/v1/releases/{owner_sentinel}/artwork"),
        format!("/api/v1/recordings/{sentinel_recording}/artwork"),
    ] {
        let kind = if path.contains("/recordings/") {
            "artwork-fetches"
        } else {
            "release-artwork-fetches"
        };
        let queued = library
            .exchange(Some(&owner), "POST", &format!("{path}/fetch"), Value::Null)
            .await;
        assert_eq!(queued.status, StatusCode::ACCEPTED);
        run_one(library).await;
        let ready = library
            .get(
                &owner,
                &format!("/api/v1/{kind}/{}", queued.json()["id"].as_str().unwrap()),
            )
            .await;
        assert!(ready["status"] == "ready" && ready["attempts"] == 1 && ready["error"].is_null());
        let image = library
            .exchange(Some(&owner), "GET", &path, Value::Null)
            .await;
        assert_eq!(image.status, StatusCode::OK);
        sentinels.push((owner.clone(), path, image.bytes));
    }
    assert!(
        sentinels[0].2 != sentinels[1].2,
        "The protected ready and manual recording images must be separate assets"
    );
    let credentials = json!({"username":"peer", "password":"owned thousand-cover peer password"});
    assert_eq!(
        library
            .exchange(Some(&owner), "POST", "/api/v1/users", credentials.clone())
            .await
            .status,
        StatusCode::CREATED
    );
    let logged = library
        .exchange(None, "POST", "/api/v1/login", credentials)
        .await;
    assert_eq!(logged.status, StatusCode::OK);
    let peer = logged.json()["token"].as_str().unwrap().to_owned();
    disable_automatic(library, &peer).await;
    let peer_installation = connect(library, &peer, &offered).await;
    let peer_sentinel_playlist = import(library, &peer, &peer_installation, "sentinel").await;
    let peer_catalog = records(library, &peer).await;
    assert_eq!(peer_catalog.len(), 1);
    let peer_sentinel = peer_catalog[0]["release_id"].as_str().unwrap().to_owned();
    assert!(
        peer_sentinel != owner_sentinel,
        "Accounts must keep separate libraries and releases"
    );
    let path = format!("/api/v1/releases/{peer_sentinel}/artwork");
    let queued = library
        .exchange(Some(&peer), "POST", &format!("{path}/fetch"), Value::Null)
        .await;
    assert_eq!(queued.status, StatusCode::ACCEPTED);
    run_one(library).await;
    let ready = library
        .get(
            &peer,
            &format!(
                "/api/v1/release-artwork-fetches/{}",
                queued.json()["id"].as_str().unwrap()
            ),
        )
        .await;
    assert!(ready["status"] == "ready" && ready["attempts"] == 1 && ready["error"].is_null());
    let image = library
        .exchange(Some(&peer), "GET", &path, Value::Null)
        .await;
    assert_eq!(image.status, StatusCode::OK);
    sentinels.push((peer.clone(), path, image.bytes));
    let storage = library.inspect(&account).await;
    assert_eq!(
        (
            storage.account_assets,
            storage.global_assets,
            storage.pending,
            storage.reserved
        ),
        (2, 3, 0, 0)
    );
    assert_eq!(
        storage.account_bytes as usize,
        sentinels[0].2.len() + sentinels[1].2.len()
    );
    assert_eq!(
        storage.global_bytes as usize,
        sentinels.iter().map(|s| s.2.len()).sum::<usize>()
    );
    storage.report("protected_start");
    library.assert_originals();
    Setup {
        owner,
        peer,
        account,
        installed,
        owner_sentinel,
        peer_sentinel,
        sentinel_recording,
        owner_sentinel_playlist,
        peer_sentinel_playlist,
        sentinels,
        storage,
    }
}

async fn exercise(library: &Library, images: &Corpus, setup: &Setup) -> usize {
    let import_started = Instant::now();
    let playlist = import(library, &setup.owner, &setup.installed, "bulk").await;
    let all = records(library, &setup.owner).await;
    assert_eq!(all.len(), COUNT + 1);
    let mut indexed = HashMap::new();
    for record in &all {
        let title = record["inherited_title"].as_str().unwrap();
        let index: usize = title
            .strip_prefix("Owned capacity recording ")
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            indexed
                .insert(
                    index,
                    (
                        record["id"].as_str().unwrap().to_owned(),
                        record["release_id"].as_str().unwrap().to_owned()
                    )
                )
                .is_none()
        );
    }
    assert_eq!(indexed.len(), COUNT + 1);
    let releases: HashSet<_> = indexed.values().map(|(_, release)| release).collect();
    assert_eq!(releases.len(), COUNT + 1);
    assert!(
        indexed[&COUNT].0 == setup.sentinel_recording && indexed[&COUNT].1 == setup.owner_sentinel
    );
    let actions = [
        json!({"kind":"recording_title_set", "recording_id":indexed[&0].0, "title":"My protected owned title"}),
        json!({"kind":"like_set", "recording_id":indexed[&0].0, "liked":true}),
        json!({"kind":"progress_set", "recording_id":indexed[&0].0, "position_ms":4200, "speed":1.25}),
    ];
    let operations: Vec<_> = actions
        .into_iter()
        .map(|action| json!({"id":Uuid::new_v4(), "action":action}))
        .collect();
    let changed = library
        .exchange(
            Some(&setup.owner),
            "POST",
            "/api/v1/sync",
            json!({"device_id":"owned-thousand", "operations":operations}),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert!(
        changed.json()["acknowledged"]
            == json!(
                operations
                    .iter()
                    .map(|op| op["id"].clone())
                    .collect::<Vec<_>>()
            )
    );
    let before = snapshot(library, setup, &playlist, &indexed[&0].0).await;
    let entries = before["playlist"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), COUNT);
    assert_eq!(
        entries
            .iter()
            .map(|v| v["recording_id"].as_str().unwrap())
            .collect::<HashSet<_>>()
            .len(),
        COUNT
    );
    assert_eq!(
        library.provider().requests_for("v1/playlists/items").len(),
        12
    );
    eprintln!(
        "artwork_thousand stage=import distinct_albums={} distinct_recordings={} elapsed_ms={}",
        releases.len() - 1,
        indexed.len() - 1,
        import_started.elapsed().as_millis()
    );
    let after_import = library.inspect(&setup.account).await;
    assert!(
        (
            after_import.account_bytes,
            after_import.global_bytes,
            after_import.reserved
        ) == (setup.storage.account_bytes, setup.storage.global_bytes, 0)
    );
    let before_calls = library.provider().requests_for("v1/artwork").len();
    for (token, release) in [
        (&setup.peer, &indexed[&0].1),
        (&setup.owner, &setup.peer_sentinel),
    ] {
        for (method, suffix) in [("GET", ""), ("POST", "/fetch")] {
            let denied = library
                .exchange(
                    Some(token),
                    method,
                    &format!("/api/v1/releases/{release}/artwork{suffix}"),
                    Value::Null,
                )
                .await;
            assert_eq!(denied.status, StatusCode::NOT_FOUND);
            private(&denied);
        }
    }
    assert_eq!(
        library.provider().requests_for("v1/artwork").len(),
        before_calls
    );
    let mut retained_sizes = vec![None; COUNT];
    let mut encoded = HashSet::new();
    let mut decoded = HashSet::new();
    let mut captured = Vec::<[u8; 32]>::new();
    let mut maximum_admission = setup.storage.account_bytes + 2 * 1024 * 1024;
    let mut refused = 0;
    let publication_started = Instant::now();
    for index in 0..COUNT {
        let state = library.inspect(&setup.account).await;
        assert_eq!((state.pending, state.reserved), (0, 0));
        maximum_admission = maximum_admission.max(state.account_bytes + 2 * 1024 * 1024);
        let path = format!("/api/v1/releases/{}/artwork", indexed[&index].1);
        let queued = library
            .exchange(
                Some(&setup.owner),
                "POST",
                &format!("{path}/fetch"),
                Value::Null,
            )
            .await;
        private(&queued);
        if queued.status == StatusCode::CONFLICT {
            assert!(
                queued.json()["code"] == "artwork_storage_full"
                    && queued.json()["error"]
                        .as_str()
                        .is_some_and(|error| error.to_lowercase().contains("storage")),
                "Only the composed typed local-storage refusal is capacity evidence"
            );
            refused = queued.status.as_u16();
            let unchanged = library.inspect(&setup.account).await;
            assert!(
                (
                    unchanged.account_assets,
                    unchanged.account_bytes,
                    unchanged.global_bytes,
                    unchanged.pending,
                    unchanged.reserved
                ) == (
                    state.account_assets,
                    state.account_bytes,
                    state.global_bytes,
                    0,
                    0
                )
            );
            break;
        }
        assert_eq!(queued.status, StatusCode::ACCEPTED);
        assert!(queued.json()["status"] == "queued");
        let admitted = library.inspect(&setup.account).await;
        assert!(
            (
                admitted.account_bytes,
                admitted.global_bytes,
                admitted.pending,
                admitted.reserved,
                admitted.account_pending,
                admitted.account_reserved
            ) == (
                state.account_bytes,
                state.global_bytes,
                1,
                2 * 1024 * 1024,
                1,
                2 * 1024 * 1024
            )
        );
        run_one(library).await;
        let ready = library
            .get(
                &setup.owner,
                &format!(
                    "/api/v1/release-artwork-fetches/{}",
                    queued.json()["id"].as_str().unwrap()
                ),
            )
            .await;
        assert!(
            ready["status"] == "ready" && ready["attempts"] == 1 && ready["error"].is_null(),
            "An accepted owned source must publish once without a hidden retry"
        );
        let image = library
            .exchange(Some(&setup.owner), "GET", &path, Value::Null)
            .await;
        assert_eq!(image.status, StatusCode::OK);
        private(&image);
        let hash: [u8; 32] = Sha256::digest(&image.bytes).into();
        assert!(
            encoded.insert(hash),
            "Every album must retain distinct encoded artwork"
        );
        assert!(
            decoded.insert(images.verify(index, &image.bytes)),
            "Every album must retain distinct decoded artwork"
        );
        captured.push(hash);
        retained_sizes[index] = Some(image.bytes.len());
        let published = library.inspect(&setup.account).await;
        assert!(
            (
                published.account_assets,
                published.account_bytes,
                published.global_assets,
                published.global_bytes,
                published.pending,
                published.reserved
            ) == (
                state.account_assets + 1,
                state.account_bytes + image.bytes.len() as i64,
                state.global_assets + 1,
                state.global_bytes + image.bytes.len() as i64,
                0,
                0
            ),
            "Real distinct publications must account for their actual retained bytes"
        );
    }
    eprintln!(
        "artwork_thousand stage=publication ready={} refused_http={} elapsed_ms={}",
        captured.len(),
        refused,
        publication_started.elapsed().as_millis()
    );
    corpus::report("retained_observed", &retained_sizes);
    let reread_started = Instant::now();
    for (index, expected) in captured.iter().enumerate() {
        let path = format!("/api/v1/releases/{}/artwork", indexed[&index].1);
        let image = library
            .exchange(Some(&setup.owner), "GET", &path, Value::Null)
            .await;
        assert_eq!(image.status, StatusCode::OK);
        private(&image);
        assert!(
            <[u8; 32]>::from(Sha256::digest(&image.bytes)) == *expected,
            "Every admitted image must survive all later publications exactly"
        );
        images.verify(index, &image.bytes);
        let denied = library
            .exchange(Some(&setup.peer), "GET", &path, Value::Null)
            .await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
        private(&denied);
    }
    for (token, path, bytes) in &setup.sentinels {
        let image = library
            .exchange(Some(token), "GET", path, Value::Null)
            .await;
        assert_eq!(image.status, StatusCode::OK);
        private(&image);
        assert!(
            image.bytes == *bytes,
            "Existing ready and explicitly selected images must remain byte-for-byte unchanged"
        );
    }
    assert!(
        snapshot(library, setup, &playlist, &indexed[&0].0).await == before,
        "Library records, playlist order, private title, likes, progress and peer state must remain unchanged"
    );
    library.assert_originals();
    let final_state = library.inspect(&setup.account).await;
    assert_eq!((final_state.pending, final_state.reserved), (0, 0));
    for reference in &setup.storage.exact_references {
        assert!(
            final_state.exact_references.contains(reference),
            "Protected sentinel references must remain exact"
        );
    }
    let mut target_hashes = HashSet::new();
    for index in 0..captured.len() {
        let rows: Vec<_> = final_state
            .references
            .iter()
            .filter(|(owner, release, _)| owner == &setup.account && release == &indexed[&index].1)
            .collect();
        assert_eq!(rows.len(), 1);
        assert!(
            target_hashes.insert(rows[0].2.clone()),
            "Each admitted album must have its own distinct retained artifact"
        );
    }
    assert_eq!(final_state.references.len(), captured.len() + 2);
    let calls = library.provider().requests_for("v1/artwork");
    assert_eq!(calls.len(), before_calls + captured.len());
    for (index, call) in calls[before_calls..].iter().enumerate() {
        assert!(
            call.json()
                == json!({"entity":"release", "provider_id":format!("owned:capacity/release/{index}")}),
            "The SDK boundary must receive exactly one expected release artwork request per admitted album"
        );
    }
    assert_eq!(library.provider().unexpected(), 0);
    assert_eq!(encoded.len(), captured.len());
    assert_eq!(decoded.len(), captured.len());
    library.audit_asset_keys().await;
    final_state.report("preserved_end");
    eprintln!(
        "artwork_thousand stage=preservation observed_complete={} distinct_retained={} largest_prefix_plus_sentinels_reservation={} headroom_to_32mib={} headroom_to_global_after_peer={} elapsed_ms={}",
        usize::from(captured.len() == COUNT),
        captured.len(),
        maximum_admission,
        32 * 1024 * 1024 - maximum_admission,
        256 * 1024 * 1024
            - (setup.storage.global_bytes - setup.storage.account_bytes)
            - maximum_admission,
        reread_started.elapsed().as_millis()
    );
    captured.len()
}

async fn disable_automatic(library: &Library, token: &str) {
    assert_eq!(
        library
            .exchange(
                Some(token),
                "PUT",
                "/api/v1/metadata/automation",
                json!({"automatic_artwork":false})
            )
            .await
            .status,
        StatusCode::OK
    );
}

async fn connect(library: &Library, token: &str, offered: &Value) -> Value {
    let installed = library
        .exchange(
            Some(token),
            "POST",
            "/api/v1/plugins",
            json!({"service_id":offered["id"], "service_revision":offered["revision"]}),
        )
        .await;
    assert_eq!(installed.status, StatusCode::CREATED);
    installed.json()
}

async fn import(library: &Library, token: &str, installed: &Value, kind: &str) -> String {
    let imported = library.exchange(Some(token), "POST", &format!("/api/v1/plugins/{}/playlists/import", installed["id"].as_str().unwrap()),
        json!({"request_id":Uuid::new_v4(), "installation_revision":installed["revision"], "playlist_reference":format!("https://provider.invalid/capacity/{kind}")})).await;
    assert_eq!(imported.status, StatusCode::OK);
    imported.json()["playlist_id"].as_str().unwrap().to_owned()
}

async fn run_one(library: &Library) {
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            library
                .hito()
                .catalog
                .run_next_artwork_job_until(std::future::pending())
        )
        .await
        .expect("One owned artwork job must finish within the ordinary request deadline")
        .unwrap()
    );
}

async fn records(library: &Library, token: &str) -> Vec<Value> {
    let first = library.get(token, "/api/v1/recordings?limit=200").await;
    let total = first["total"].as_u64().unwrap() as usize;
    let mut result = first["items"].as_array().unwrap().clone();
    for offset in (200..total).step_by(200) {
        let page = library
            .get(
                token,
                &format!("/api/v1/recordings?limit=200&offset={offset}"),
            )
            .await;
        assert_eq!(page["total"].as_u64().unwrap() as usize, total);
        result.extend(page["items"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(result.len(), total);
    result
}

async fn snapshot(library: &Library, setup: &Setup, playlist: &str, recording: &str) -> Value {
    let progress = library
        .exchange(
            Some(&setup.owner),
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids":[recording]}),
        )
        .await;
    assert_eq!(progress.status, StatusCode::OK);
    json!({"recordings": records(library, &setup.owner).await,
        "playlist":library.get(&setup.owner, &format!("/api/v1/playlists/{playlist}")).await,
        "owner_playlists":library.get(&setup.owner, "/api/v1/playlists").await,
        "owner_sentinel_playlist":library.get(&setup.owner, &format!("/api/v1/playlists/{}", setup.owner_sentinel_playlist)).await,
        "peer_sentinel_playlist":library.get(&setup.peer, &format!("/api/v1/playlists/{}", setup.peer_sentinel_playlist)).await,
        "metadata":library.get(&setup.owner, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources":library.get(&setup.owner, &format!("/api/v1/recordings/{recording}/sources")).await,
        "progress":progress.json(), "events":library.get(&setup.owner, "/api/v1/sync?cursor=0").await,
        "peer_recordings":records(library, &setup.peer).await,
        "peer_playlists":library.get(&setup.peer, "/api/v1/playlists").await,
        "peer_plugins":library.get(&setup.peer, "/api/v1/plugins").await,
        "peer_events":library.get(&setup.peer, "/api/v1/sync?cursor=0").await})
}

fn private(response: &crate::artwork_thousand_http::Response) {
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(response.headers["x-request-id"].to_str().unwrap()).is_ok());
}
