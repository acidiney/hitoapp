use hito_core::test_support::provider_audio as audio_fixture;

use audio_fixture::AudioLibrary;
use hito_core::{
    AudioRequest, CoreConfig, Credentials, DownloadRequest, DownloadState, HostDeviceId,
    OpenedAudio,
};
use std::{future::pending, path::Path};
use uuid::Uuid;

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn staging_name() -> String {
    format!("{}.{}.part", Uuid::new_v4(), Uuid::new_v4())
}

#[tokio::test]
async fn startup_removes_orphaned_staging_files_and_keeps_published_audio_and_originals() {
    let fixture = AudioLibrary::start().await;
    fixture.ready().await;
    fixture
        .hito
        .downloads
        .create(
            &fixture.actor,
            &fixture.source.recording_id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert!(
        fixture
            .hito
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    let AudioLibrary {
        directory,
        music,
        original,
        source,
        hito,
        ..
    } = fixture;
    let owned = directory.path().join("hito.downloads");
    let published: Vec<String> = names(&owned);
    assert_eq!(published.len(), 1);
    assert!(published[0].ends_with(".media"));
    let published_bytes = std::fs::read(owned.join(&published[0])).unwrap();
    let orphans = [staging_name(), staging_name()];
    for orphan in &orphans {
        std::fs::write(owned.join(orphan), b"half written audio").unwrap();
    }
    let stray_media = format!("{}.{}.media", Uuid::new_v4(), Uuid::new_v4());
    std::fs::write(owned.join(&stray_media), b"unknown published audio").unwrap();
    std::fs::write(owned.join("notes.txt"), b"not a download").unwrap();
    drop(hito);

    let hito = hito_core::open(CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("provider-audio").unwrap(),
    ))
    .await
    .unwrap();

    let mut expected = vec![
        published[0].clone(),
        stray_media.clone(),
        "notes.txt".to_owned(),
    ];
    expected.sort();
    assert_eq!(names(&owned), expected);
    assert_eq!(
        std::fs::read(owned.join(&published[0])).unwrap(),
        published_bytes
    );
    assert_eq!(std::fs::read(music.join("original.wav")).unwrap(), original);

    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let ready = hito
        .downloads
        .status(&actor, &source.recording_id)
        .await
        .unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    let OpenedAudio::Content(mut audio) = hito
        .playback
        .open_audio(&actor, &ready.source_id.unwrap(), AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("the retained download must open");
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, original);
}
