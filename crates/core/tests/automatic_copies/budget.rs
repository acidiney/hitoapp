use super::fixture::Copies;
use hito_core::{AutomaticCopyPreference, DownloadState};

#[tokio::test]
async fn eviction_removes_the_lowest_scored_copy_and_spares_downloads_announced_and_playing_copies()
{
    let f = Copies::start().await;
    let size = f.original.len() as u64;
    let kept = f.recording("kept").await;
    let skipped = f.recording("skipped").await;
    let recent = f.recording("recent").await;
    let fifth = f.recording("fifth").await;
    let downloaded = f.recording("downloaded").await;
    f.download(&downloaded).await;
    f.budget(size * 5 / 2).await;
    f.announce(&f.actor, &[&kept, &skipped]).await;
    f.drain().await;
    assert!(f.copy_source(&kept).await.is_some());
    assert!(f.copy_source(&skipped).await.is_some());
    let me = f.actor.user().id.clone();
    f.sql(&format!(
        "INSERT INTO listening_history(user_id,recording_id,played_at,play_count) VALUES
            ('{me}','{kept}',unixepoch()-30*86400,6),('{me}','{recent}',unixepoch()-86400,0)"
    ))
    .await;

    f.announce(&f.actor, &[&recent]).await;
    f.drain().await;
    assert!(f.copy_source(&skipped).await.is_none());
    assert!(f.copy_source(&kept).await.is_some());
    assert!(f.copy_source(&recent).await.is_some());
    assert_eq!(f.managed_files().len(), 3);
    assert_eq!(
        f.hito
            .downloads
            .automatic_copies(&f.actor)
            .await
            .unwrap()
            .used_bytes,
        2 * size
    );

    f.sql(&format!(
        "INSERT INTO listening_progress(user_id,recording_id,position_ms) VALUES('{me}','{recent}',1000)"
    ))
    .await;
    f.announce(&f.actor, &[&fifth]).await;
    f.drain().await;
    assert!(f.copy_source(&kept).await.is_none());
    assert!(f.copy_source(&recent).await.is_some());
    assert!(f.copy_source(&fifth).await.is_some());

    f.download(&fifth).await;
    f.announce(&f.actor, &[]).await;
    f.sql("DELETE FROM listening_progress").await;
    f.budget(0).await;
    assert!(f.copy_source(&recent).await.is_none());
    assert!(f.copy_source(&fifth).await.is_some());
    for recording in [&fifth, &downloaded] {
        assert!(matches!(
            f.hito
                .downloads
                .status(&f.actor, recording)
                .await
                .unwrap()
                .status,
            DownloadState::Ready
        ));
    }
    assert_eq!(f.managed_files().len(), 2);
    assert_eq!(
        f.hito
            .downloads
            .automatic_copies(&f.actor)
            .await
            .unwrap()
            .used_bytes,
        0
    );
}

#[tokio::test]
async fn an_evicted_copy_falls_back_to_the_provider_source() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
    f.announce(&f.actor, &[&next]).await;
    f.drain().await;
    let copy = f.copy_source(&next).await.unwrap();
    let plugin = f.plugin_source(&next).await;
    let granted = f
        .hito
        .playback
        .playback_source(&f.actor, &next, &copy)
        .await
        .unwrap();
    granted.check().await.unwrap();

    f.announce(&f.actor, &[]).await;
    f.budget(0).await;
    assert!(granted.check().await.is_err());
    assert!(f.copy_source(&next).await.is_none());
    assert!(f.managed_files().is_empty());
    let mut candidates = f
        .hito
        .playback
        .playback_candidates(&f.actor, &next)
        .await
        .unwrap();
    assert_eq!(
        candidates.next().await.unwrap().unwrap().source_id(),
        plugin
    );
    assert_eq!(
        f.count("SELECT count(*) FROM sources WHERE provider='download'")
            .await,
        0
    );
}

#[tokio::test]
async fn turning_keep_played_tracks_off_removes_only_automatic_copies() {
    let f = Copies::start().await;
    let automatic = f.recording("automatic").await;
    let both = f.recording("both").await;
    let downloaded = f.recording("downloaded").await;
    f.announce(&f.actor, &[&automatic, &both]).await;
    f.drain().await;
    f.download(&both).await;
    f.download(&downloaded).await;
    assert_eq!(f.managed_files().len(), 3);

    let off = f
        .hito
        .downloads
        .set_automatic_copies(
            &f.actor,
            AutomaticCopyPreference {
                keep_played_tracks: false,
            },
        )
        .await
        .unwrap();
    assert!(!off.keep_played_tracks);
    assert_eq!(off.used_bytes, 0);
    assert!(f.copy_source(&automatic).await.is_none());
    for recording in [&both, &downloaded] {
        assert!(matches!(
            f.hito
                .downloads
                .status(&f.actor, recording)
                .await
                .unwrap()
                .status,
            DownloadState::Ready
        ));
    }
    assert_eq!(f.managed_files().len(), 2);
    assert_eq!(f.count("SELECT count(*) FROM automatic_copies").await, 0);

    f.announce(&f.actor, &[&automatic]).await;
    assert_eq!(f.drain().await, 0);
    assert!(f.copy_source(&automatic).await.is_none());
}
