#[path = "fixtures/provider_artwork.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Actor, CoreConfig, Credentials, Error, HostDeviceId, ImportProviderPlaylist,
    PluginInstallation, Services, open,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn saved_private_recording_retains_artwork_without_reimporting_or_changing_collection_state()
{
    let server = fixture::ArtworkServer::start().await;
    let (directory, hito, owner, shared_library, music) =
        support::library("provider-artwork").await;
    let installed = server.connect(&hito, &owner).await;
    let first = import_copy(&hito, &owner, &installed).await;
    let second = import_copy(&hito, &owner, &installed).await;
    let first_detail = hito
        .collection
        .playlist(&owner, &first.to_string())
        .await
        .unwrap();
    let second_detail = hito
        .collection
        .playlist(&owner, &second.to_string())
        .await
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(first_detail.entries.len(), 2);
    assert_eq!(second_detail.entries.len(), 2);
    let recording = first_detail.entries[0].recording_id.clone().unwrap();
    assert!(
        first_detail
            .entries
            .iter()
            .chain(&second_detail.entries)
            .all(|entry| entry.recording_id.as_deref() == Some(recording.as_str()))
    );
    let entry_ids = first_detail
        .entries
        .iter()
        .chain(&second_detail.entries)
        .map(|entry| &entry.entry_id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(entry_ids.len(), 4);
    let recording_id = Uuid::parse_str(&recording).unwrap();
    support::synchronize(
        &hito,
        &owner,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My title".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My artist".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::PlaylistRename {
                playlist_id: first,
                name: "My first copy".into(),
            },
        ],
    )
    .await;
    hito.identity
        .create_user(
            &owner,
            Credentials {
                username: "other".into(),
                password: "other artwork password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "other".into(),
            password: "other artwork password".into(),
        })
        .await
        .unwrap();
    let other = hito.identity.authenticate(&session.token).await.unwrap();
    let writer = reopen(&directory, &music).await;
    let before = saved_state(&hito, &owner, [first, second], &recording).await;
    assert_ne!(before["catalog"]["items"][0]["library_id"], shared_library);
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["sources"], json!([]));
    assert_eq!(before["metadata"]["title"]["personal"], "My title");
    assert_eq!(before["metadata"]["artist"]["personal"], "My artist");
    assert_eq!(before["catalog"]["items"][0]["liked"], true);
    assert_eq!(before["position_ms"], 4_200);
    assert_eq!(before["first"]["playlist"]["name"], "My first copy");
    assert_eq!(before["second"]["playlist"]["name"], "Owned selection");
    assert_eq!(
        before["metadata"]["selected_observation"]["provider_recording_id"],
        fixture::PROVIDER_RECORDING_ID
    );
    let requests_before = server.requests().await;
    assert!(
        hito.catalog
            .artwork(&owner, &recording)
            .await
            .unwrap()
            .is_none()
    );
    let queued = hito
        .catalog
        .enqueue_recording_artwork(&owner, &recording)
        .await
        .unwrap();
    let queued = serde_json::to_value(queued).unwrap();
    assert_eq!(queued["status"], "queued");
    let job_id = queued["id"].as_str().unwrap().to_owned();
    assert_eq!(
        serde_json::to_value(hito.catalog.artwork_fetch(&owner, &job_id).await.unwrap()).unwrap(),
        queued
    );
    assert_eq!(
        server.requests().await,
        requests_before,
        "reads and enqueue must not contact the provider"
    );
    assert!(matches!(
        hito.catalog.artwork_fetch(&other, &job_id).await,
        Err(Error::NotFound)
    ));

    let worker = {
        let catalog = hito.catalog.clone();
        tokio::spawn(async move {
            catalog
                .run_next_artwork_job_until(std::future::pending::<()>())
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), server.image_entered())
        .await
        .expect("the owned provider must start its image body");
    let unrelated_playlist = Uuid::new_v4();
    tokio::time::timeout(
        Duration::from_secs(3),
        support::synchronize(
            &writer,
            &other,
            vec![Action::PlaylistCreate {
                playlist_id: unrelated_playlist,
                name: "Concurrent write".into(),
            }],
        ),
    )
    .await
    .expect("artwork I/O must not hold a SQLite write transaction");
    assert!(
        !worker.is_finished(),
        "the image must remain held during the independent write"
    );
    server.release_image();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .expect("the released artwork job must finish")
            .unwrap()
            .unwrap()
    );
    let ready =
        serde_json::to_value(hito.catalog.artwork_fetch(&owner, &job_id).await.unwrap()).unwrap();
    assert_eq!(ready["id"], job_id);
    assert_eq!(ready["status"], "ready");
    assert_eq!(
        saved_state(&hito, &owner, [first, second], &recording).await,
        before
    );
    let requests_after = server.requests().await;
    assert_eq!(&requests_after[..requests_before.len()], &requests_before);
    assert_eq!(
        &requests_after[requests_before.len()..],
        &[json!({
            "method": "POST", "path": "v1/artwork",
            "body": {"entity": "recording", "provider_id": fixture::PROVIDER_RECORDING_ID}
        })],
        "artwork must not refetch playlist, metadata, sources, or audio"
    );
    drop(writer);
    drop(hito);
    drop(server);

    let reopened = reopen(&directory, &music).await;
    let artwork = reopened
        .catalog
        .artwork(&owner, &recording)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(artwork.content_type, "image/png");
    assert_eq!(artwork.bytes, fixture::COVER);
    assert!(matches!(
        reopened.catalog.artwork(&other, &recording).await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        serde_json::to_value(
            reopened
                .catalog
                .artwork_fetch(&owner, &job_id)
                .await
                .unwrap()
        )
        .unwrap()["status"],
        "ready"
    );
    assert_eq!(
        saved_state(&reopened, &owner, [first, second], &recording).await,
        before
    );
    reopened
        .plugins
        .remove(&owner, &installed.id, installed.revision)
        .await
        .unwrap();
    assert_eq!(
        reopened
            .catalog
            .artwork(&owner, &recording)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        fixture::COVER
    );
    assert_eq!(
        saved_state(&reopened, &owner, [first, second], &recording).await,
        before
    );
}

async fn import_copy(hito: &Services, owner: &Actor, installed: &PluginInstallation) -> Uuid {
    hito.collection
        .import_provider_playlist(
            owner,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-selection".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id
}

async fn reopen(directory: &tempfile::TempDir, music: &std::path::Path) -> Services {
    open(CoreConfig::new(
        directory.path().join("hito.db"),
        music.to_owned(),
        HostDeviceId::new("provider-artwork").unwrap(),
    ))
    .await
    .unwrap()
}

async fn saved_state(
    hito: &Services,
    owner: &Actor,
    playlists: [Uuid; 2],
    recording: &str,
) -> Value {
    json!({
        "catalog": hito.catalog.recordings(owner, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(owner).await.unwrap(),
        "first": hito.collection.playlist(owner, &playlists[0].to_string()).await.unwrap(),
        "second": hito.collection.playlist(owner, &playlists[1].to_string()).await.unwrap(),
        "sources": hito.catalog.sources(owner, recording).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(owner, recording, Default::default()).await.unwrap(),
        "activity": hito.collection.activity(owner).await.unwrap(),
        "events": hito.collection.events(owner, 0).await.unwrap(),
        "position_ms": hito.collection.saved_position(owner, Uuid::parse_str(recording).unwrap())
            .await.unwrap().unwrap().position_ms
    })
}
