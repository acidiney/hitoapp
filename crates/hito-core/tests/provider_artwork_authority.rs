#[path = "fixtures/provider_artwork.rs"]
mod fixture;
mod support;

use hito_core::{
    Actor, ArtworkFetchStatus, CoreConfig, Credentials, Error, HostDeviceId,
    ImportProviderPlaylist, PluginInstallation, Services, open,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[tokio::test]
async fn logout_during_an_image_body_cancels_the_session_job_without_publishing_artwork() {
    let case = saved_recording().await;
    let before = saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await;
    let requests = case.server.requests().await;
    let job = case
        .hito
        .catalog
        .enqueue_recording_artwork(&case.owner, &case.recording)
        .await
        .unwrap();
    let worker = start_worker(&case.hito);
    image_entered(&case.server).await;

    case.hito
        .identity
        .logout(&case.session_token)
        .await
        .unwrap();
    finish_worker(worker).await;
    assert!(matches!(
        case.hito.catalog.artwork_fetch(&case.owner, &job.id).await,
        Err(Error::Unauthorized)
    ));
    let (_, owner) = login(&case.hito).await;
    assert!(!owner.same_session(&case.owner));
    let cancelled = case
        .hito
        .catalog
        .artwork_fetch(&owner, &job.id)
        .await
        .unwrap();
    assert!(matches!(cancelled.status, ArtworkFetchStatus::Cancelled));
    case.server.release_image();
    assert!(
        case.hito
            .catalog
            .artwork(&owner, &case.recording)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved_state(&case.hito, &owner, case.playlist, &case.recording).await,
        before
    );
    assert!(
        !case
            .hito
            .catalog
            .run_next_artwork_job_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_artwork_requests(&case.server, &requests, 1).await;
}

#[tokio::test]
async fn provider_removal_during_an_image_body_cancels_the_job_without_publishing_artwork() {
    let case = saved_recording().await;
    let before = saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await;
    let requests = case.server.requests().await;
    let job = case
        .hito
        .catalog
        .enqueue_recording_artwork(&case.owner, &case.recording)
        .await
        .unwrap();
    let worker = start_worker(&case.hito);
    image_entered(&case.server).await;

    case.hito
        .plugins
        .remove(&case.owner, &case.installed.id, case.installed.revision)
        .await
        .unwrap();
    finish_worker(worker).await;
    let cancelled = case
        .hito
        .catalog
        .artwork_fetch(&case.owner, &job.id)
        .await
        .unwrap();
    assert!(matches!(cancelled.status, ArtworkFetchStatus::Cancelled));
    case.server.release_image();
    assert!(
        case.hito
            .catalog
            .artwork(&case.owner, &case.recording)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await,
        before
    );
    assert!(
        !case
            .hito
            .catalog
            .run_next_artwork_job_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_artwork_requests(&case.server, &requests, 1).await;
}

#[tokio::test]
async fn queued_artwork_resumes_after_reopen_with_its_live_session_and_remains_readable_offline() {
    let case = saved_recording().await;
    let before = saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await;
    let requests = case.server.requests().await;
    let queued = case
        .hito
        .catalog
        .enqueue_recording_artwork(&case.owner, &case.recording)
        .await
        .unwrap();
    assert!(matches!(queued.status, ArtworkFetchStatus::Queued));
    assert_eq!(queued.attempts, 0);
    assert_eq!(case.server.requests().await, requests);
    drop(case.hito);

    let reopened = reopen(case.directory.path(), &case.music).await;
    let queued_after_restart = reopened
        .catalog
        .artwork_fetch(&case.owner, &queued.id)
        .await
        .unwrap();
    assert!(matches!(
        queued_after_restart.status,
        ArtworkFetchStatus::Queued
    ));
    assert_eq!(queued_after_restart.attempts, 0);
    let worker = start_worker(&reopened);
    image_entered(&case.server).await;
    case.server.release_image();
    finish_worker(worker).await;
    let ready = reopened
        .catalog
        .artwork_fetch(&case.owner, &queued.id)
        .await
        .unwrap();
    assert!(matches!(ready.status, ArtworkFetchStatus::Ready));
    assert_eq!(ready.attempts, 1);
    assert_artwork_requests(&case.server, &requests, 1).await;
    drop(case.server);

    let artwork = reopened
        .catalog
        .artwork(&case.owner, &case.recording)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(artwork.content_type, "image/png");
    assert_eq!(artwork.bytes, fixture::COVER);
    assert_eq!(
        saved_state(&reopened, &case.owner, case.playlist, &case.recording).await,
        before
    );
}

#[tokio::test]
async fn shutdown_joins_a_held_image_job_and_requeues_it_without_consuming_an_attempt() {
    let case = saved_recording().await;
    let before = saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await;
    let requests = case.server.requests().await;
    let job = case
        .hito
        .catalog
        .enqueue_recording_artwork(&case.owner, &case.recording)
        .await
        .unwrap();
    let (stop, shutdown) = tokio::sync::oneshot::channel::<()>();
    let worker = {
        let catalog = case.hito.catalog.clone();
        tokio::spawn(async move {
            catalog
                .run_next_artwork_job_until(async {
                    let _ = shutdown.await;
                })
                .await
        })
    };
    image_entered(&case.server).await;

    stop.send(()).unwrap();
    finish_worker(worker).await;
    let interrupted = case
        .hito
        .catalog
        .artwork_fetch(&case.owner, &job.id)
        .await
        .unwrap();
    assert!(matches!(interrupted.status, ArtworkFetchStatus::Queued));
    assert_eq!(interrupted.attempts, 0);
    assert!(interrupted.error.is_none());
    assert!(
        case.hito
            .catalog
            .artwork(&case.owner, &case.recording)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await,
        before
    );
    assert_artwork_requests(&case.server, &requests, 1).await;

    case.server.release_image();
    case.server.release_image();
    finish_worker(start_worker(&case.hito)).await;
    let ready = case
        .hito
        .catalog
        .artwork_fetch(&case.owner, &job.id)
        .await
        .unwrap();
    assert!(matches!(ready.status, ArtworkFetchStatus::Ready));
    assert_eq!(ready.attempts, 1);
    assert_eq!(
        case.hito
            .catalog
            .artwork(&case.owner, &case.recording)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        fixture::COVER
    );
    assert_eq!(
        saved_state(&case.hito, &case.owner, case.playlist, &case.recording).await,
        before
    );
    assert_artwork_requests(&case.server, &requests, 2).await;
}

struct SavedRecording {
    server: fixture::ArtworkServer,
    directory: tempfile::TempDir,
    music: PathBuf,
    hito: Services,
    owner: Actor,
    session_token: String,
    installed: PluginInstallation,
    recording: String,
    playlist: Uuid,
}

async fn saved_recording() -> SavedRecording {
    let server = fixture::ArtworkServer::start().await;
    let (directory, hito, _setup_actor, _shared_library, music) =
        support::library("artwork-authority").await;
    let (session_token, owner) = login(&hito).await;
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
    SavedRecording {
        server,
        directory,
        music,
        hito,
        owner,
        session_token,
        installed,
        recording,
        playlist,
    }
}

async fn login(hito: &Services) -> (String, Actor) {
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let owner = hito.identity.authenticate(&session.token).await.unwrap();
    (session.token, owner)
}

async fn reopen(directory: &Path, music: &Path) -> Services {
    open(CoreConfig::new(
        directory.join("hito.db"),
        music.to_owned(),
        HostDeviceId::new("artwork-authority").unwrap(),
    ))
    .await
    .unwrap()
}

fn start_worker(hito: &Services) -> JoinHandle<hito_core::Result<bool>> {
    let catalog = hito.catalog.clone();
    tokio::spawn(async move {
        catalog
            .run_next_artwork_job_until(std::future::pending())
            .await
    })
}

async fn image_entered(server: &fixture::ArtworkServer) {
    tokio::time::timeout(Duration::from_secs(3), server.image_entered())
        .await
        .expect("the owned provider must start its held image body");
}

async fn finish_worker(worker: JoinHandle<hito_core::Result<bool>>) {
    assert!(
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .expect("the artwork worker must join promptly")
            .unwrap()
            .unwrap()
    );
}

async fn assert_artwork_requests(server: &fixture::ArtworkServer, before: &[Value], count: usize) {
    let after = server.requests().await;
    assert_eq!(&after[..before.len()], before);
    let expected = vec![
        json!({
            "method": "POST", "path": "v1/artwork",
            "body": {"entity": "recording", "provider_id": fixture::PROVIDER_RECORDING_ID}
        });
        count
    ];
    assert_eq!(&after[before.len()..], expected.as_slice());
}

async fn saved_state(hito: &Services, owner: &Actor, playlist: Uuid, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(owner, Default::default()).await.unwrap(),
        "playlist": hito.collection.playlist(owner, &playlist.to_string()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(owner, recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(owner, recording).await.unwrap(),
        "events": hito.collection.events(owner, 0).await.unwrap()
    })
}
