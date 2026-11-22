use super::fixture::{Copies, DURATION_MS};
use hito_core::{Action, AutomaticCopyPreference, Error, Operation, SyncBatch};
use uuid::Uuid;

const NINETY_PERCENT: i64 = DURATION_MS * 9 / 10;

#[tokio::test]
async fn a_full_play_counts_once_and_saves_a_copy() {
    let f = Copies::start().await;
    let heard = f.recording("heard").await;
    f.listen(
        &f.actor,
        &heard,
        &[10_000, 100_000, NINETY_PERCENT, 195_000, DURATION_MS],
    )
    .await;
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(1));
    assert!(f.drain().await > 0);
    assert!(f.copy_source(&heard).await.is_some());
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);

    f.listen(&f.actor, &heard, &[10_000, DURATION_MS]).await;
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(2));
    assert_eq!(f.drain().await, 0);
    assert_eq!(f.server.requests("v1/audio").await.len(), 1);
    assert_eq!(f.managed_files().len(), 1);
}

#[tokio::test]
async fn a_partial_play_neither_counts_nor_saves() {
    let f = Copies::start().await;
    let heard = f.recording("heard").await;
    f.listen(&f.actor, &heard, &[10_000, 100_000, NINETY_PERCENT - 1])
        .await;
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(0));
    assert_eq!(f.drain().await, 0);
    assert!(f.copy_source(&heard).await.is_none());
    assert!(f.server.requests("v1/audio").await.is_empty());
}

#[tokio::test]
async fn a_full_play_saves_nothing_while_keep_played_tracks_is_off() {
    let f = Copies::start().await;
    let heard = f.recording("heard").await;
    f.hito
        .downloads
        .set_automatic_copies(
            &f.actor,
            AutomaticCopyPreference {
                keep_played_tracks: false,
            },
        )
        .await
        .unwrap();
    f.listen(&f.actor, &heard, &[10_000, DURATION_MS]).await;
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(1));
    assert_eq!(f.drain().await, 0);
    assert!(f.copy_source(&heard).await.is_none());
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert!(f.managed_files().is_empty());
}

#[tokio::test]
async fn a_full_play_with_history_paused_saves_a_copy_without_counting() {
    let f = Copies::start().await;
    let heard = f.recording("heard").await;
    f.hito
        .collection
        .set_history_enabled(&f.actor, false)
        .await
        .unwrap();
    f.listen(&f.actor, &heard, &[10_000, DURATION_MS]).await;
    assert_eq!(f.play_count(&f.actor, &heard).await, None);
    assert!(f.drain().await > 0);
    assert!(f.copy_source(&heard).await.is_some());
}

#[tokio::test]
async fn a_full_play_changes_nothing_for_another_account() {
    let f = Copies::start().await;
    let heard = f.recording("heard").await;
    let member = f.member().await;
    f.listen(&f.actor, &heard, &[DURATION_MS]).await;
    f.drain().await;
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(1));
    assert!(f.copy_source(&heard).await.is_some());

    let replayed = f
        .hito
        .collection
        .apply(
            &member,
            SyncBatch {
                device_id: "member-phone".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::ProgressSet {
                        recording_id: Uuid::parse_str(&heard).unwrap(),
                        position_ms: DURATION_MS,
                        speed: 1.0,
                    },
                }],
            },
        )
        .await;
    assert!(matches!(replayed, Err(Error::NotFound)));
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(1));
    assert_eq!(f.play_count(&member, &heard).await, None);
    let member_id = member.user().id.clone();
    for table in ["automatic_copies", "download_intents"] {
        assert_eq!(
            f.count(&format!(
                "SELECT count(*) FROM {table} WHERE account_id='{member_id}'"
            ))
            .await,
            0
        );
    }
    assert_eq!(
        f.hito
            .downloads
            .automatic_copies(&member)
            .await
            .unwrap()
            .used_bytes,
        0
    );
}

#[tokio::test]
async fn eviction_removes_the_less_played_copy_first() {
    let f = Copies::start().await;
    let size = f.original.len() as u64;
    let favourite = f.recording("favourite").await;
    let once = f.recording("once").await;
    let next = f.recording("next").await;
    f.budget(size * 2).await;
    f.announce(&f.actor, &[&favourite, &once]).await;
    f.drain().await;
    f.sql(&format!(
        "UPDATE automatic_copies SET created_at=created_at-60 WHERE recording_id='{favourite}'"
    ))
    .await;
    f.listen(
        &f.actor,
        &favourite,
        &[10_000, DURATION_MS, 10_000, DURATION_MS],
    )
    .await;
    f.listen(&f.actor, &once, &[10_000, DURATION_MS]).await;
    assert_eq!(f.play_count(&f.actor, &favourite).await, Some(2));
    assert_eq!(f.play_count(&f.actor, &once).await, Some(1));

    f.announce_playing(&f.actor, Some(&next), &[]).await;
    f.drain().await;
    assert!(f.copy_source(&next).await.is_some());
    assert!(f.copy_source(&favourite).await.is_some());
    assert!(f.copy_source(&once).await.is_none());
    assert_eq!(f.managed_files().len(), 2);
}

#[tokio::test]
async fn a_full_play_of_a_playback_only_plugin_track_is_never_copied() {
    let f = Copies::with_policy(Some("playback_only")).await;
    let heard = f.recording("heard").await;
    f.listen(&f.actor, &heard, &[10_000, DURATION_MS]).await;
    assert_eq!(f.play_count(&f.actor, &heard).await, Some(1));
    assert_eq!(f.drain().await, 1);
    assert_eq!(f.drain().await, 0);
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert_eq!(f.count("SELECT count(*) FROM download_intents").await, 0);
    assert_eq!(
        f.count("SELECT count(*) FROM played_copy_requests WHERE next_attempt_at>unixepoch()")
            .await,
        1
    );
}
