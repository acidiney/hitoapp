#![cfg(unix)]

use hito_core::test_support::media_tool;
use hito_core::test_support::support;

use hito_core::{CoreConfig, HostDeviceId, Search};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use std::time::Duration;

#[tokio::test]
async fn a_file_swap_during_inspection_cannot_import_audio_or_tags_outside_the_music_folder() {
    let (dir, _original, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    support::write_wav(&path, 0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Authorized audio".into());
    tag.save_to_path(&path, Default::default()).unwrap();
    let outside = dir.path().join("private.wav");
    support::write_wav(&outside, 1);
    tag.set_title("PRIVATE OUTSIDE AUDIO".into());
    tag.save_to_path(&outside, Default::default()).unwrap();
    let saved = dir.path().join("saved.wav");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "mv {path} {saved}\nln -s {outside} {path}\n/usr/bin/ffprobe \"$@\"\nresult=$?\nrm {path}\nmv {saved} {path}\nexit \"$result\"",
            path = media_tool::quote(&path),
            saved = media_tool::quote(&saved),
            outside = media_tool::quote(&outside),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = tool;
    let hito = hito_core::open(config).await.unwrap();
    support::scan(&hito, &actor, &library).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].title, "Authorized audio");
    assert!(recordings.items[0].available);
}

#[test]
fn cancelling_large_file_verification_does_not_delay_runtime_shutdown() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (_dir, hito, actor, library, music) = support::library("server").await;
            let path = music.join("large.wav");
            support::write_wav(&path, 0);
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(8 * 1024 * 1024 * 1024)
                .unwrap();
            hito.storage
                .enqueue_scan(&actor, &library)
                .await
                .unwrap()
                .remove(0);
            let indexer = hito.storage.clone();
            let worker = tokio::spawn(async move { indexer.run_next_job().await });
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(
                !worker.is_finished(),
                "The large input must still be under verification"
            );
            worker.abort();
            assert!(worker.await.unwrap_err().is_cancelled());
        });
        drop(runtime);
        sender.send(()).unwrap();
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("Cancelled file readers must stop promptly before runtime shutdown");
}

#[tokio::test]
async fn a_scan_retains_its_selected_directory_when_the_root_path_changes_during_inspection() {
    let (dir, _original, actor, library, music) = support::library("server").await;
    support::write_wav(&music.join("track.wav"), 0);
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    support::write_wav(&outside.join("track.wav"), 1);
    let marker = dir.path().join("probe-started");
    let resume = dir.path().join("resume");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "touch {}\nwhile [ ! -e {} ]; do sleep 0.01; done\nexec /usr/bin/ffprobe \"$@\"",
            media_tool::quote(&marker),
            media_tool::quote(&resume),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = tool;
    let hito = hito_core::open(config).await.unwrap();
    hito.storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    let indexer = hito.storage.clone();
    let worker = tokio::spawn(async move { indexer.run_next_job().await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let selected = dir.path().join("selected");
    std::fs::rename(&music, &selected).unwrap();
    std::os::unix::fs::symlink(outside, &music).unwrap();
    std::fs::write(&resume, "").unwrap();
    assert!(worker.await.unwrap().unwrap());
    std::fs::remove_file(&music).unwrap();
    std::fs::rename(selected, &music).unwrap();
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert!(recordings.items[0].available);
}
