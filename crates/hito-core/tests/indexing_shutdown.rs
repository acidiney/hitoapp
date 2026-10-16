#[cfg(unix)]
#[path = "fixtures/media_tool.rs"]
mod media_tool;
mod support;
use std::future::ready;

#[tokio::test]
async fn shutdown_requeues_an_unstarted_scan_and_releases_its_write_transaction() {
    let (directory, core, actor, library, music) = support::library("shutdown-test").await;
    support::write_wav(&music.join("fixture.wav"), 0);
    let job = core
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(core.storage.run_next_job_until(ready(())).await.unwrap());
    assert_eq!(
        core.storage.job(&actor, &job.id).await.unwrap().state,
        "queued"
    );
    let reopened = hito_core::open(hito_core::CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("shutdown-test").unwrap(),
    ))
    .await
    .unwrap();
    assert!(reopened.storage.run_next_job().await.unwrap());
    assert_eq!(
        core.storage.job(&actor, &job.id).await.unwrap().state,
        "complete"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn stopping_inspection_preserves_verified_sources_and_requeues_without_holding_a_writer() {
    use hito_core::{CoreConfig, HostDeviceId, Search};
    use std::time::Duration;
    let (directory, original, actor, library, music) = support::library("shutdown-test").await;
    support::write_wav(&music.join("original.wav"), 0);
    support::scan(&original, &actor, &library).await;
    let recording = original
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    support::write_wav(&music.join("new.wav"), 1);
    let marker = directory.path().join("inspection");
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("shutdown-test").unwrap(),
    );
    config.media_tools.ffprobe = media_tool::script(
        directory.path(),
        "ffprobe",
        &format!(
            "printf '%s\\n' \"$$\" > {}\nexec sleep 300",
            media_tool::quote(&marker)
        ),
    );
    let core = hito_core::open(config).await.unwrap();
    let job = core
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    tokio::time::timeout(
        Duration::from_secs(5),
        core.storage.run_next_job_until(async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }),
    )
    .await
    .unwrap()
    .unwrap();
    core.storage.finish_indexing().await.unwrap();
    assert_eq!(
        core.storage.job(&actor, &job.id).await.unwrap().state,
        "queued"
    );
    let current = core
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, recording.id);
    assert!(current.items[0].available);
    let restarted = hito_core::open(CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("shutdown-test").unwrap(),
    ))
    .await
    .unwrap();
    assert!(restarted.storage.run_next_job().await.unwrap());
    assert_eq!(
        restarted
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .total,
        2
    );
}
