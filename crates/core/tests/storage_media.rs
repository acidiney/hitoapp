#![cfg(unix)]
use hito_core::test_support::support;

use hito_core::{Error, Search};

#[tokio::test]
async fn playback_keeps_its_opened_file_when_the_path_is_replaced() {
    let (dir, services, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    let expected = support::write_wav(&path, 0);
    let outside = dir.path().join("outside.wav");
    support::write_wav(&outside, 1);
    support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = services
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    let opened = services
        .catalog
        .audio_file(&actor, &source.id)
        .await
        .unwrap();
    std::fs::rename(&path, music.join("saved.wav")).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(
        support::audio_bytes(opened) == expected,
        "Playback must retain the file it opened"
    );
    assert!(matches!(
        services.catalog.audio_file(&actor, &source.id).await,
        Err(Error::NotFound)
    ));
}
