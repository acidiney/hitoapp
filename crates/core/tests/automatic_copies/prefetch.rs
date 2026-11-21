use super::fixture::Copies;
use hito_core::{
    AutomaticCopies, AutomaticCopyPreference, DiskSpace, Error,
    test_support::{hang_guard::hang_guard, playback::LocalPlayback},
};

const DEFAULT_BUDGET: u64 = 5 * 1024 * 1024 * 1024;

#[tokio::test]
async fn an_announced_track_is_saved_once_and_playback_picks_the_copy_first() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
    f.announce(&f.actor, &[&next]).await;
    assert!(f.drain().await > 0);

    let copy = f
        .copy_source(&next)
        .await
        .expect("a published automatic copy");
    let mut candidates = f
        .hito
        .playback
        .playback_candidates(&f.actor, &next)
        .await
        .unwrap();
    assert_eq!(candidates.next().await.unwrap().unwrap().source_id(), copy);
    assert_eq!(
        f.hito.downloads.automatic_copies(&f.actor).await.unwrap(),
        AutomaticCopies {
            keep_played_tracks: true,
            used_bytes: f.original.len() as u64,
            budget_bytes: DEFAULT_BUDGET,
        }
    );
    assert!(matches!(
        f.hito.downloads.status(&f.actor, &next).await,
        Err(Error::NotFound)
    ));
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);

    f.announce(&f.actor, &[&next]).await;
    assert_eq!(f.drain().await, 0);
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);
    assert_eq!(f.managed_files().len(), 1);
}

#[tokio::test]
async fn nothing_is_saved_while_keep_played_tracks_is_off() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
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
    f.announce(&f.actor, &[&next]).await;
    assert_eq!(f.drain().await, 0);
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert!(f.copy_source(&next).await.is_none());
    assert!(f.managed_files().is_empty());

    f.hito
        .downloads
        .set_automatic_copies(
            &f.actor,
            AutomaticCopyPreference {
                keep_played_tracks: true,
            },
        )
        .await
        .unwrap();
    assert!(f.drain().await > 0);
    assert!(f.copy_source(&next).await.is_some());
}

#[tokio::test]
async fn low_disk_space_stops_new_copies_before_the_fetch_and_before_publishing() {
    let f = Copies::start().await;
    let first = f.recording("first").await;
    let second = f.recording("second").await;
    f.hito.downloads.simulate_disk_space(Some(DiskSpace {
        available: 9,
        total: 100,
    }));
    f.announce(&f.actor, &[&first]).await;
    assert_eq!(f.drain().await, 0);
    assert!(f.server.requests("v1/audio").await.is_empty());

    f.hito.downloads.simulate_disk_space(Some(DiskSpace {
        available: 50,
        total: 100,
    }));
    assert!(f.drain().await > 0);
    assert!(f.copy_source(&first).await.is_some());

    let gate = f.server.hold("audio-tail").await;
    f.announce(&f.actor, &[&second]).await;
    let downloads = f.hito.downloads.clone();
    let worker = tokio::spawn(async move { super::fixture::drain_owned(downloads).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), gate.entered())
        .await
        .expect("the second fetch must start");
    f.hito.downloads.simulate_disk_space(Some(DiskSpace {
        available: 5,
        total: 100,
    }));
    gate.open();
    tokio::time::timeout(std::time::Duration::from_secs(20), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(f.copy_source(&second).await.is_none());
    assert_eq!(f.managed_files().len(), 1);
    assert_eq!(
        f.count("SELECT count(*) FROM download_jobs WHERE status='queued' AND next_attempt_at>unixepoch()")
            .await,
        1
    );
}

#[tokio::test]
async fn a_playback_only_plugin_is_never_copied() {
    let f = Copies::with_policy(Some("playback_only")).await;
    let next = f.recording("next").await;
    f.announce(&f.actor, &[&next]).await;
    assert_eq!(f.drain().await, 1);
    assert_eq!(f.drain().await, 0);
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert!(f.copy_source(&next).await.is_none());
    assert_eq!(f.count("SELECT count(*) FROM download_intents").await, 0);
    assert_eq!(f.count("SELECT count(*) FROM automatic_copies").await, 0);
}

#[tokio::test]
async fn local_originals_are_never_copied() {
    let local = hang_guard("the local library must open", LocalPlayback::start()).await;
    local
        .services
        .downloads
        .announce_upcoming(
            &local.actor,
            hito_core::UpcomingRecordings {
                client: None,
                recording_ids: vec![local.recording.clone()],
            },
        )
        .await
        .unwrap();
    assert_eq!(super::fixture::drain(&local.services).await, 0);
    let sources = local
        .services
        .catalog
        .sources(&local.actor, &local.recording)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].provider, "local");
    assert!(std::fs::read(local.music.join("track.wav")).unwrap() == local.original);
}

#[tokio::test]
async fn a_busy_provider_is_retried_with_backoff_without_spending_attempts() {
    let f = Copies::start().await;
    let next = f.recording("next").await;
    f.server.audio_statuses(vec![
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
    ]);
    f.announce(&f.actor, &[&next]).await;
    f.drain().await;
    assert_eq!(
        f.count("SELECT busy_retries*10+attempts FROM download_jobs")
            .await,
        10
    );
    assert_eq!(
        f.count("SELECT next_attempt_at-unixepoch() BETWEEN 1 AND 5 FROM download_jobs")
            .await,
        1
    );
    f.sql("UPDATE download_jobs SET next_attempt_at=0").await;
    f.drain().await;
    assert_eq!(
        f.count("SELECT busy_retries*10+attempts FROM download_jobs")
            .await,
        20
    );
    assert_eq!(
        f.count("SELECT next_attempt_at-unixepoch() BETWEEN 6 AND 10 FROM download_jobs")
            .await,
        1
    );
    f.sql("UPDATE download_jobs SET next_attempt_at=0").await;
    f.drain().await;
    assert!(f.copy_source(&next).await.is_some());
    assert_eq!(f.server.requests("v1/audio").await.len(), 3);
}
