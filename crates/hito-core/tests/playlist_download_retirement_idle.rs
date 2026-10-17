#[path = "fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use audio_fixture::AudioLibrary;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, Credentials, DownloadRequest, DownloadState, Error, PlaylistDownloadEntryState,
    PlaylistDownloadStatus,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, path::PathBuf, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_ready_copy_retained_by_one_playlist_leaves_reconciliation_idle_until_removal() {
    let library = AudioLibrary::start().await;
    library.ready().await;
    let outcome = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.hito.downloads.finish_downloads().await.unwrap();
    assert!(
        std::fs::read(library.music.join("original.wav")).unwrap() == library.original,
        "retention changes must preserve the user-owned original bytes"
    );
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(library: &AudioLibrary) {
    let hito = &library.hito;
    let actor = &library.actor;
    let recording = &library.source.recording_id;
    let recording_id = Uuid::parse_str(recording).unwrap();
    let playlists = [Uuid::new_v4(), Uuid::new_v4()];
    let mut actions = Vec::new();
    for (index, playlist_id) in playlists.iter().enumerate() {
        actions.push(Action::PlaylistCreate {
            playlist_id: *playlist_id,
            name: format!("Owned retained playlist {index}"),
        });
        actions.push(Action::EntryInsert {
            playlist_id: *playlist_id,
            entry_id: Uuid::new_v4(),
            recording_id,
            position: 0,
        });
    }
    actions.extend([
        Action::RecordingTitleSet {
            recording_id,
            title: "My retained title".into(),
        },
        Action::RecordingArtistSet {
            recording_id,
            artist: "My retained artist".into(),
        },
        Action::LikeSet {
            recording_id,
            liked: true,
        },
        Action::ProgressSet {
            recording_id,
            position_ms: 700,
            speed: 1.25,
        },
    ]);
    support::synchronize(hito, actor, actions).await;
    let credentials = || Credentials {
        username: "other".into(),
        password: "owned other account password".into(),
    };
    hito.identity
        .create_user(actor, credentials())
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    let other = hito.identity.authenticate(&session.token).await.unwrap();
    let direct = hito
        .downloads
        .create(actor, recording, request())
        .await
        .unwrap();
    assert!(hito.downloads.run_next_job_until(pending()).await.unwrap());
    let direct_status = hito.downloads.status(actor, recording).await.unwrap();
    assert!(matches!(direct_status.status, DownloadState::Ready));
    assert_eq!(direct_status.intent_id, direct.intent_id);
    let source = direct_status.source_id.unwrap();
    assert_ne!(source, library.source.id);
    let mut roots = Vec::new();
    for playlist in playlists {
        roots.push(
            hito.downloads
                .create_playlist(actor, &playlist.to_string(), request())
                .await
                .unwrap(),
        );
    }
    assert_ne!(roots[0].retention_id, roots[1].retention_id);
    for root in &roots {
        assert_ne!(root.retention_id, direct.intent_id);
    }
    settle_membership(library, &playlists, &source).await;
    drain_reconciliation(library).await;
    for (playlist, root) in playlists.iter().zip(&roots) {
        let status = ready_playlist(library, *playlist, &source).await;
        assert_eq!(status.retention_id, root.retention_id);
    }
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert_eq!(library.server.requests("v1/audio").await.len(), 1);
    let files = owned_files(library);
    assert_eq!(files.len(), 1);
    let media = &files[0];
    assert!(std::fs::read(media).unwrap() == library.original);
    let preserved = preserved_state(library, &playlists).await;
    let grant = hito
        .catalog
        .playback_source(actor, recording, &source)
        .await
        .unwrap();
    assert!(grant.is_local());
    library.server.offline().await;

    hito.downloads
        .remove(actor, &roots[0].retention_id)
        .await
        .unwrap();
    hito.downloads
        .remove(actor, &direct.intent_id)
        .await
        .unwrap();
    drain_reconciliation(library).await;
    assert!(matches!(
        hito.downloads
            .playlist_status(actor, &playlists[0].to_string(), Default::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.downloads.status(actor, recording).await,
        Err(Error::NotFound)
    ));
    let remaining = ready_playlist(library, playlists[1], &source).await;
    assert_eq!(remaining.retention_id, roots[1].retention_id);
    grant.check().await.unwrap();
    assert!(support::audio_bytes(grant.open_file().await.unwrap()) == library.original);
    assert_eq!(owned_files(library).len(), 1);
    assert!(media.is_file());
    assert!(std::fs::read(media).unwrap() == library.original);
    assert_private(library, &other, &playlists, &source).await;
    assert!(preserved_state(library, &playlists).await == preserved);
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert!(
        !hito
            .downloads
            .run_next_playlist_reconciliation_until(pending())
            .await
            .unwrap()
    );

    tokio::time::sleep(Duration::from_secs(6)).await;
    let progressed = hito
        .downloads
        .run_next_playlist_reconciliation_until(pending())
        .await
        .unwrap();
    assert!(
        !progressed,
        "a settled ready copy kept by an unchanged playlist must not requeue retirement work every five seconds"
    );
    assert_eq!(
        ready_playlist(library, playlists[1], &source).await,
        remaining
    );
    grant.check().await.unwrap();
    assert!(support::audio_bytes(grant.open_file().await.unwrap()) == library.original);
    assert!(media.is_file());
    assert_eq!(library.server.requests("v1/audio").await.len(), 1);
    assert!(preserved_state(library, &playlists).await == preserved);

    hito.downloads
        .remove(actor, &roots[1].retention_id)
        .await
        .unwrap();
    drain_reconciliation(library).await;
    assert!(grant.check().await.is_err());
    assert!(hito.downloads.run_next_job_until(pending()).await.unwrap());
    hito.downloads.finish_downloads().await.unwrap();
    assert!(
        !media.exists(),
        "the final retention removal must reclaim the actual managed media file"
    );
    assert!(
        owned_files(library).is_empty(),
        "the owned download store must contain no abandoned media or staging bytes"
    );
    assert!(matches!(
        hito.downloads
            .playlist_status(actor, &playlists[1].to_string(), Default::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog.audio_file(actor, &source).await,
        Err(Error::NotFound)
    ));
    let sources = hito.catalog.sources(actor, recording).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, library.source.id);
    assert_eq!(sources[0].provider, "plugin");
    assert_private(library, &other, &playlists, &source).await;
    assert!(preserved_state(library, &playlists).await == preserved);
    assert_eq!(library.server.requests("v1/audio").await.len(), 1);
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert!(
        !hito
            .downloads
            .run_next_playlist_reconciliation_until(pending())
            .await
            .unwrap()
    );
}

fn request() -> DownloadRequest {
    DownloadRequest {
        request_id: Uuid::new_v4(),
    }
}

async fn settle_membership(library: &AudioLibrary, playlists: &[Uuid; 2], source: &str) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            library
                .hito
                .downloads
                .run_next_playlist_reconciliation_until(pending())
                .await
                .unwrap();
            let mut complete = true;
            for playlist in playlists {
                let status = library
                    .hito
                    .downloads
                    .playlist_status(&library.actor, &playlist.to_string(), Default::default())
                    .await
                    .unwrap();
                assert_eq!(status.entries.items.len(), 1);
                complete &= status.desired_revision == status.reconciled_revision
                    && status.entries.items[0].status == PlaylistDownloadEntryState::Ready
                    && status.entries.items[0].source_id.as_deref() == Some(source);
            }
            if complete {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both original playlist projections must reconcile before idle is measured");
}

async fn drain_reconciliation(library: &AudioLibrary) {
    for _ in 0..32 {
        if !library
            .hito
            .downloads
            .run_next_playlist_reconciliation_until(pending())
            .await
            .unwrap()
        {
            return;
        }
    }
    panic!("the small owned reconciliation workflow must drain to public idle");
}

async fn ready_playlist(
    library: &AudioLibrary,
    playlist: Uuid,
    source: &str,
) -> PlaylistDownloadStatus {
    let status = library
        .hito
        .downloads
        .playlist_status(&library.actor, &playlist.to_string(), Default::default())
        .await
        .unwrap();
    assert_eq!(status.desired_revision, status.reconciled_revision);
    assert_eq!(status.entries.items.len(), 1);
    let entry = &status.entries.items[0];
    assert_eq!(
        entry.recording_id.as_deref(),
        Some(library.source.recording_id.as_str())
    );
    assert_eq!(entry.status, PlaylistDownloadEntryState::Ready);
    assert_eq!(entry.source_id.as_deref(), Some(source));
    status
}

async fn assert_private(
    library: &AudioLibrary,
    other: &Actor,
    playlists: &[Uuid; 2],
    source: &str,
) {
    assert_eq!(
        library
            .hito
            .catalog
            .recordings(other, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert!(
        library
            .hito
            .collection
            .playlists(other)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        library
            .hito
            .plugins
            .settings(other)
            .await
            .unwrap()
            .installations
            .is_empty()
    );
    assert!(matches!(
        library
            .hito
            .catalog
            .sources(other, &library.source.recording_id)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        library.hito.catalog.audio_file(other, source).await,
        Err(Error::NotFound)
    ));
    for playlist in playlists {
        assert!(matches!(
            library
                .hito
                .downloads
                .playlist_status(other, &playlist.to_string(), Default::default())
                .await,
            Err(Error::NotFound)
        ));
    }
}

async fn preserved_state(library: &AudioLibrary, playlists: &[Uuid; 2]) -> Value {
    let hito = &library.hito;
    let actor = &library.actor;
    let metadata = hito
        .catalog
        .recording_metadata(actor, &library.source.recording_id, Default::default())
        .await
        .unwrap();
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "libraries": hito.catalog.libraries(actor).await.unwrap(),
        "first": hito.collection.playlist(actor, &playlists[0].to_string()).await.unwrap(),
        "second": hito.collection.playlist(actor, &playlists[1].to_string()).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "metadata": {"title": metadata.title, "artist": metadata.artist}
    })
}

fn owned_files(library: &AudioLibrary) -> Vec<PathBuf> {
    let owned = library.directory.path().join("hito.downloads");
    let files: Vec<_> = std::fs::read_dir(&owned)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    for file in &files {
        assert!(file.parent() == Some(owned.as_path()));
        assert!(file.is_file());
    }
    files
}
