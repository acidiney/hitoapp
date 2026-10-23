use crate::{
    Action, Actor, CoreConfig, Credentials, HostDeviceId, Operation, Services, SyncBatch, open,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub async fn library(host: &str) -> (tempfile::TempDir, Services, Actor, String, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = open(CoreConfig::new(
        dir.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new(host).unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let library = hito.catalog.libraries(&actor).await.unwrap().remove(0).id;
    (dir, hito, actor, library, music)
}

pub async fn scan(hito: &Services, actor: &Actor, library: &str) {
    let job = hito
        .storage
        .enqueue_scan(actor, library)
        .await
        .unwrap()
        .remove(0);
    assert!(hito.storage.run_next_job().await.unwrap());
    let job = hito.storage.job(actor, &job.id).await.unwrap();
    assert_eq!(job.state, "complete", "{:?}", job.error);
}

pub fn write_wav(path: &Path, sample_offset: i16) -> Vec<u8> {
    let sample_count = 8000u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + sample_count * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(sample_count * 2).to_le_bytes());
    for sample in 0..sample_count {
        bytes.extend_from_slice(&((sample % 100) as i16 * 100 + sample_offset).to_le_bytes());
    }
    std::fs::write(path, &bytes).unwrap();
    bytes
}

pub async fn catalog() -> (tempfile::TempDir, Services, Actor, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    for (offset, title) in ["Alpha", "alpha", "beta", "Bravo", "zulu"]
        .into_iter()
        .enumerate()
    {
        write_wav(&music.join(format!("{title}.wav")), offset as i16);
    }
    let hito = open(CoreConfig::new(
        dir.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "catalog test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let library = hito.catalog.libraries(&actor).await.unwrap().remove(0).id;
    let job = hito
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(hito.storage.run_next_job().await.unwrap());
    assert_eq!(
        hito.storage.job(&actor, &job.id).await.unwrap().state,
        "complete"
    );
    (dir, hito, actor, library)
}

pub async fn synchronize(hito: &Services, actor: &Actor, actions: Vec<Action>) {
    hito.collection
        .apply(
            actor,
            SyncBatch {
                device_id: "catalog-test".into(),
                operations: actions
                    .into_iter()
                    .map(|action| Operation {
                        id: Uuid::new_v4(),
                        action,
                    })
                    .collect(),
            },
        )
        .await
        .unwrap();
}

pub fn audio_bytes(mut audio: crate::AudioFile) -> Vec<u8> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut audio.file, &mut bytes).unwrap();
    bytes
}
