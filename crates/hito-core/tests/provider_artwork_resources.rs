#[path = "fixtures/provider_artwork.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{
    Actor, ArtworkFetchStatus, Credentials, ImportProviderPlaylist, InstallPlugin,
    PluginInstallation, ProviderSearch, Services,
};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn current_cached_artwork_keeps_its_ready_receipt_after_history_cleanup_and_provider_removal()
{
    let server = fixture::ArtworkServer::start().await;
    let (directory, hito, owner, _shared, _music) = support::library("artwork-history").await;
    let installed = server.connect(&hito, &owner).await;
    let recording = import_recording(&hito, &owner, &installed).await;
    let before = saved_state(&hito, &owner, &recording).await;
    let job = hito
        .catalog
        .enqueue_recording_artwork(&owner, &recording)
        .await
        .unwrap();
    let worker = {
        let catalog = hito.catalog.clone();
        tokio::spawn(async move {
            catalog
                .run_next_artwork_job_until(std::future::pending())
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), server.image_entered())
        .await
        .unwrap();
    server.release_image();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    let ready = hito.catalog.artwork_fetch(&owner, &job.id).await.unwrap();
    assert!(matches!(ready.status, ArtworkFetchStatus::Ready));
    assert_eq!(
        hito.catalog
            .artwork(&owner, &recording)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        fixture::COVER
    );
    let requests = server.requests().await;

    let mut clock = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let aged = sqlx::query("UPDATE artwork_fetches SET updated_at = unixepoch() - 8 * 86400 WHERE id = ? AND status = 'ready'")
        .bind(&job.id).execute(&mut clock).await.unwrap().rows_affected();
    assert_eq!(aged, 1);
    clock.close().await.unwrap();
    assert!(
        !hito
            .catalog
            .run_next_artwork_job_until(std::future::pending())
            .await
            .unwrap()
    );
    hito.plugins
        .remove(&owner, &installed.id, installed.revision)
        .await
        .unwrap();

    let retained = hito
        .catalog
        .enqueue_recording_artwork(&owner, &recording)
        .await
        .expect("the current retained cover must keep its ready receipt after history cleanup");
    assert!(matches!(retained.status, ArtworkFetchStatus::Ready));
    assert_eq!(retained.id, ready.id);
    assert_eq!(retained.attempts, 1);
    let artwork = hito
        .catalog
        .artwork(&owner, &recording)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(artwork.content_type, "image/png");
    assert_eq!(artwork.bytes, fixture::COVER);
    assert_eq!(saved_state(&hito, &owner, &recording).await, before);
    assert_eq!(server.requests().await, requests);
}

#[tokio::test]
async fn four_background_artwork_downloads_leave_provider_search_and_local_artwork_responsive() {
    let server = fixture::ArtworkServer::start_with_search().await;
    let (_directory, hito, owner, shared, music) = support::library("artwork-admission").await;
    support::write_wav(&music.join("local.wav"), 10);
    std::fs::write(music.join("cover.png"), fixture::COVER).unwrap();
    support::scan(&hito, &owner, &shared).await;
    let local_recording = hito
        .catalog
        .recordings(&owner, Default::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let installed = server.connect(&hito, &owner).await;
    let service_id = installed.service_id.clone();
    let service_revision = hito
        .plugins
        .settings(&owner)
        .await
        .unwrap()
        .services
        .into_iter()
        .find(|service| service.id == service_id)
        .unwrap()
        .revision;
    let mut accounts = vec![(owner.clone(), installed)];
    for index in 1..5 {
        let username = format!("artwork{index}");
        hito.identity
            .create_user(
                &owner,
                Credentials {
                    username: username.clone(),
                    password: "owned background password".into(),
                },
            )
            .await
            .unwrap();
        let session = hito
            .identity
            .login(Credentials {
                username,
                password: "owned background password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let installed = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service_id.clone(),
                    service_revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        accounts.push((actor, installed));
    }
    for (actor, installed) in accounts.iter().take(4) {
        let recording = import_recording(&hito, actor, installed).await;
        hito.catalog
            .enqueue_recording_artwork(actor, &recording)
            .await
            .unwrap();
    }
    let mut workers = Vec::new();
    for _ in 0..4 {
        let catalog = hito.catalog.clone();
        workers.push(tokio::spawn(async move {
            catalog
                .run_next_artwork_job_until(std::future::pending())
                .await
        }));
    }
    for _ in 0..4 {
        tokio::time::timeout(Duration::from_secs(3), server.image_entered())
            .await
            .expect("all four accounts must be downloading held image bodies");
    }

    let (same_account_search, other_account_search, local_artwork) = tokio::join!(
        search_finishes(&hito, &accounts[0].0, &accounts[0].1),
        search_finishes(&hito, &accounts[4].0, &accounts[4].1),
        local_artwork_finishes(&hito, &owner, &local_recording),
    );
    for _ in 0..4 {
        server.release_image();
    }
    let mut completed = Vec::new();
    for worker in workers {
        completed.push(
            tokio::time::timeout(Duration::from_secs(3), worker)
                .await
                .expect("released background artwork workers must join")
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(
        (same_account_search, other_account_search, local_artwork),
        (true, true, true),
        "background image downloads must preserve one-second interactive access"
    );
    assert!(completed.into_iter().all(|claimed| claimed));
    let searches = server
        .requests()
        .await
        .into_iter()
        .filter(|request| request["path"] == "v1/search")
        .count();
    assert_eq!(searches, 2);
}

async fn search_finishes(hito: &Services, actor: &Actor, installed: &PluginInstallation) -> bool {
    tokio::time::timeout(
        Duration::from_secs(1),
        hito.catalog.search_provider(
            actor,
            &installed.id,
            ProviderSearch {
                installation_revision: installed.revision,
                query: "owned fixture query".into(),
                cursor: None,
                limit: 10,
            },
        ),
    )
    .await
    .is_ok_and(|result| {
        result.is_ok_and(|page| page.items.is_empty() && page.next_cursor.is_none())
    })
}

async fn local_artwork_finishes(hito: &Services, actor: &Actor, recording: &str) -> bool {
    tokio::time::timeout(
        Duration::from_secs(1),
        hito.catalog.artwork(actor, recording),
    )
    .await
    .is_ok_and(|result| {
        result.is_ok_and(|artwork| artwork.is_some_and(|artwork| artwork.bytes == fixture::COVER))
    })
}

async fn import_recording(
    hito: &Services,
    actor: &Actor,
    installed: &PluginInstallation,
) -> String {
    let playlist = hito
        .collection
        .import_provider_playlist(
            actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-selection".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id;
    hito.collection
        .playlist(actor, &playlist.to_string())
        .await
        .unwrap()
        .entries[0]
        .recording_id
        .clone()
        .unwrap()
}

async fn saved_state(hito: &Services, owner: &Actor, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(owner, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(owner).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(owner, recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(owner, recording).await.unwrap(),
        "events": hito.collection.events(owner, 0).await.unwrap()
    })
}
