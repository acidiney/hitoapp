use super::fixture::{Copies, drain_owned};
use hito_core::{AudioRequest, OpenedAudio};
use std::time::Duration;

#[tokio::test]
async fn a_prefetch_yields_to_foreground_playback_and_finishes_later() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
    let playing = f.recording("playing").await;
    let plugin = f.plugin_source(&playing).await;
    let gate = f.server.hold("audio-tail").await;
    f.announce(&f.actor, &[&next]).await;
    let downloads = f.hito.downloads.clone();
    let worker = tokio::spawn(async move { drain_owned(downloads).await });
    tokio::time::timeout(Duration::from_secs(10), gate.entered())
        .await
        .expect("the prefetch must be mid-body");

    let granted = f
        .hito
        .playback
        .playback_source(&f.actor, &playing, &plugin)
        .await
        .unwrap();
    let opened = tokio::time::timeout(
        Duration::from_secs(5),
        granted.open(AudioRequest::default()),
    )
    .await
    .expect("foreground playback must not wait on the prefetch")
    .expect("foreground playback must not be refused as busy");
    assert!(matches!(opened, OpenedAudio::Content(_)));
    drop(opened);
    gate.open();
    tokio::time::timeout(Duration::from_secs(20), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(f.copy_source(&next).await.is_none());
    assert_eq!(
        f.count("SELECT count(*) FROM download_jobs WHERE status='queued' AND attempts=0 AND next_attempt_at>unixepoch()")
            .await,
        1
    );
    assert!(f.managed_files().is_empty());

    f.sql("UPDATE download_jobs SET next_attempt_at=0").await;
    f.drain().await;
    assert!(f.copy_source(&next).await.is_some());
    assert_eq!(f.server.requests("v1/audio").await.len(), 3);
}
