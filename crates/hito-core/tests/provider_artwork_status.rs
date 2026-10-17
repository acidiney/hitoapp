#[path = "fixtures/provider_artwork.rs"]
#[allow(dead_code)]
mod fixture;
mod support;

use hito_core::{ArtworkFetchStatus, ImportProviderPlaylist};
use sqlx::{Connection, Executor, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn artwork_status_remains_readable_while_an_unrelated_sqlite_writer_is_held() {
    let server = fixture::ArtworkServer::start().await;
    let (directory, hito, owner, _shared, _music) = support::library("artwork-status").await;
    let installed = server.connect(&hito, &owner).await;
    let playlist = hito
        .collection
        .import_provider_playlist(
            &owner,
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
    let recording = hito
        .collection
        .playlist(&owner, &playlist.to_string())
        .await
        .unwrap()
        .entries[0]
        .recording_id
        .clone()
        .unwrap();
    let queued = hito
        .catalog
        .enqueue_recording_artwork(&owner, &recording)
        .await
        .unwrap();
    let requests = server.requests().await;
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    writer.execute("BEGIN IMMEDIATE").await.unwrap();
    let mut polling = {
        let catalog = hito.catalog.clone();
        let actor = owner.clone();
        let id = queued.id.clone();
        tokio::spawn(async move { catalog.artwork_fetch(&actor, &id).await })
    };
    let observed = tokio::time::timeout(Duration::from_secs(1), &mut polling).await;

    writer.execute("ROLLBACK").await.unwrap();
    writer.close().await.unwrap();
    let (finished_while_writer_held, result) = match observed {
        Ok(result) => (true, result),
        Err(_) => (false, polling.await),
    };
    let receipt = result.unwrap().unwrap();

    assert!(
        finished_while_writer_held,
        "reading a private artwork receipt must not wait for an unrelated SQLite writer"
    );
    assert_eq!(receipt.id, queued.id);
    assert_eq!(receipt.recording_id, recording);
    assert!(matches!(receipt.status, ArtworkFetchStatus::Queued));
    assert_eq!(receipt.attempts, 0);
    assert!(receipt.error.is_none());
    assert_eq!(server.requests().await, requests);
}
