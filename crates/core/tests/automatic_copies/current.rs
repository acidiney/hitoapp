use super::fixture::Copies;
use std::future::pending;

#[tokio::test]
async fn the_announced_current_recording_is_kept_until_another_recording_is_current() {
    let f = Copies::start().await;
    let playing = f.recording("playing").await;
    let next = f.recording("next").await;
    f.announce_playing(&f.actor, Some(&playing), &[]).await;
    assert!(f.drain().await > 0);
    assert!(f.copy_source(&playing).await.is_some());

    f.budget(0).await;
    assert!(f.copy_source(&playing).await.is_some());
    assert_eq!(f.managed_files().len(), 1);

    f.announce_playing(&f.actor, Some(&next), &[]).await;
    f.budget(0).await;
    assert!(f.copy_source(&playing).await.is_none());
    assert!(f.managed_files().is_empty());
}

#[tokio::test]
async fn the_current_recording_is_saved_after_the_upcoming_ones() {
    let f = Copies::start().await;
    let playing = f.recording("playing").await;
    let next = f.recording("next").await;
    f.announce_playing(&f.actor, Some(&playing), &[&next]).await;

    assert!(
        f.hito
            .downloads
            .run_next_copy_until(pending())
            .await
            .unwrap()
    );
    let queued = |recording: &str| {
        format!("SELECT count(*) FROM download_intents WHERE recording_id='{recording}'")
    };
    assert_eq!(f.count(&queued(&next)).await, 1);
    assert_eq!(f.count(&queued(&playing)).await, 0);

    f.drain().await;
    assert!(f.copy_source(&next).await.is_some());
    assert!(f.copy_source(&playing).await.is_some());
    assert_eq!(f.server.requests("v1/audio").await.len(), 2);
}

#[tokio::test]
async fn recent_progress_keeps_a_copy_only_while_no_client_names_a_current_recording() {
    let f = Copies::start().await;
    let heard = f.recording("heard").await;
    let other = f.recording("other").await;
    f.announce(&f.actor, &[&heard]).await;
    f.drain().await;
    f.announce(&f.actor, &[]).await;
    f.listen(&f.actor, &heard, &[30_000]).await;

    f.budget(0).await;
    assert!(f.copy_source(&heard).await.is_some());

    f.announce_playing(&f.actor, Some(&other), &[]).await;
    f.budget(0).await;
    assert!(f.copy_source(&heard).await.is_none());
}
