#![cfg(feature = "device")]
#[path = "../fixtures/audiobook_queue.rs"]
#[allow(dead_code)]
mod audiobook;
use hito_audio::test_support::capture;
use hito_core::test_support::playback as fixture;
#[path = "../fixtures/audiobook_output.rs"]
mod output;
use hito_core::test_support::support;

use hito_core::{Action, AudiobookChapterAnchor, Error};
use hito_playback::{
    Player, PlayerAction, PlayerAudiobookRequest, PlayerCommand, PlayerError, PlayerPhase,
};
use std::time::Duration;
use tokio::sync::oneshot;
use tracing_subscriber::layer::SubscriberExt;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn continue_restores_fresh_exact_chapter_position_and_tempo_without_interrupting_preparation_or_replay()
 {
    let f = audiobook::Library::with_audio(output::write_audio).await;
    let [a, b] = f.recordings;
    let first: AudiobookChapterAnchor = serde_json::from_value(f.anchors[0].clone()).unwrap();
    let expected: AudiobookChapterAnchor = serde_json::from_value(f.anchors[2].clone()).unwrap();
    support::synchronize(
        &f.source.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: a,
                position_ms: 1_000,
                speed: 1.25,
            },
            Action::AudiobookChapterSet { chapter: first },
        ],
    )
    .await;
    let rendered = f
        .source
        .services
        .collection
        .audiobook(&f.actor, &f.book.to_string())
        .await
        .unwrap();
    assert_eq!(rendered.continuation.as_ref().unwrap().chapter, first);
    let peer_before = peer_state(&f).await;
    let signals = output::WorkflowSignals::default();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(signals.clone()));
    let capture = capture::Capture::open();
    let (player, owner) =
        tracing::dispatcher::with_default(&dispatch, || Player::native(f.source.services.clone()));
    let attached = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap();
    for (sequence, revision, action) in [
        (
            1,
            0,
            PlayerAction::Replace {
                recordings: vec![b],
                start: 0,
            },
        ),
        (2, 1, PlayerAction::Resume {}),
    ] {
        player
            .submit(
                &f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: attached.lease,
                    sequence,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
    }
    let mut state = player.subscribe(&f.actor, attached.lease).await.unwrap();
    let mut position = player
        .subscribe_position(&f.actor, attached.lease)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| state.phase == PlayerPhase::Ready && state.playing),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| position.position_ms >= 150),
    )
    .await
    .unwrap()
    .unwrap();
    hear(&capture, 0.15);
    assert_tone(&capture.recent(0.1), 220.);
    support::synchronize(
        &f.source.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: a,
                position_ms: 4_000,
                speed: 0.75,
            },
            Action::AudiobookChapterSet { chapter: expected },
        ],
    )
    .await;
    let old = state.borrow().clone();
    assert_eq!(old.current.unwrap().recording_id, b);
    assert_eq!(old.speed, 1000);
    let input = PlayerAudiobookRequest {
        id: Uuid::new_v4(),
        lease: attached.lease,
        revision: old.revision,
        selection: old.selection,
        audiobook_id: f.book,
    };
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(f.source.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    let lock = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let preparing = tokio::spawn({
        let player = player.clone();
        let actor = f.actor.clone();
        let input = input.clone();
        async move { player.prepare_audiobook(&actor, input).await }
    });
    tokio::time::timeout(Duration::from_secs(1), signals.prepared.notified())
        .await
        .unwrap();
    hear(&capture, 0.15);
    assert!(
        !preparing.is_finished(),
        "The fixed observed writer prefix must settle before preparation completes"
    );
    assert_eq!(*state.borrow(), old);
    assert_tone(&capture.recent(0.1), 220.);
    lock.rollback().await.unwrap();
    let prepared = tokio::time::timeout(Duration::from_secs(3), preparing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(prepared.chapter, expected);
    assert_eq!(
        (prepared.position_ms, prepared.speed, prepared.count),
        (4_000, 750, 3)
    );
    assert_eq!(*state.borrow(), old);

    let lock = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let cancelled = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 3,
        revision: old.revision,
        action: PlayerAction::ContinueAudiobook {
            token: prepared.token,
        },
    };
    let submitting = tokio::spawn({
        let player = player.clone();
        let actor = f.actor.clone();
        let command = cancelled.clone();
        async move { player.submit(&actor, command).await }
    });
    tokio::time::timeout(Duration::from_secs(1), signals.admitting.notified())
        .await
        .unwrap();
    assert!(!submitting.is_finished());
    hear(&capture, 0.15);
    assert_tone(&capture.recent(0.1), 220.);
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(1),
            player.settle_audiobook(&f.actor, attached.lease, input.id, cancelled.clone())
        )
        .await
        .unwrap()
        .unwrap(),
        None
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), submitting)
            .await
            .unwrap()
            .unwrap(),
        Err(PlayerError::Conflict)
    );
    assert_eq!(*state.borrow(), old);
    hear(&capture, 0.15);
    assert_tone(&capture.recent(0.1), 220.);
    lock.rollback().await.unwrap();

    let fresh = PlayerAudiobookRequest {
        id: Uuid::new_v4(),
        ..input
    };
    let prepared = player.prepare_audiobook(&f.actor, fresh).await.unwrap();
    assert_eq!(prepared.chapter, expected);
    assert_eq!(
        (prepared.position_ms, prepared.speed, prepared.count),
        (4_000, 750, 3)
    );
    let command = PlayerCommand {
        id: Uuid::new_v4(),
        lease: attached.lease,
        sequence: 4,
        revision: old.revision,
        action: PlayerAction::ContinueAudiobook {
            token: prepared.token,
        },
    };
    let target_output = capture.mark();
    let receipt = player.submit(&f.actor, command.clone()).await.unwrap();
    assert_eq!((receipt.sequence, receipt.revision), (4, old.revision + 1));
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready
                && state.playing
                && state.speed == 750
                && state
                    .current
                    .is_some_and(|current| current.chapter == Some(expected))
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let selected = state.borrow().current.unwrap();
    assert_eq!(state.borrow().current_index, Some(2));
    assert_eq!(state.borrow().queue_len, 3);
    assert!(!f.chapters.contains(&selected.id));
    let queue = player
        .queue(&f.actor, attached.lease, receipt.revision, 0, 3)
        .await
        .unwrap();
    assert_eq!(queue.entries[0].recording.id, a.to_string());
    assert_eq!(queue.entries[2].recording.id, a.to_string());
    assert_eq!(queue.entries[0].chapter, Some(first));
    assert_eq!(queue.entries[2].chapter, Some(expected));
    assert_ne!(queue.entries[0].id, selected.id);
    assert_eq!(queue.entries[2].id, selected.id);
    tokio::time::timeout(
        Duration::from_secs(3),
        position.wait_for(|position| {
            position.entry == Some(selected.id) && position.position_ms >= 4_100
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(position.borrow().duration_ms, Some(8_000));
    tokio::task::block_in_place(|| {
        capture::wait(|| output::first_chapter_start(&capture.all()[target_output..]).is_some())
    });
    assert_eq!(
        output::first_chapter_start(&capture.all()[target_output..]),
        Some(output::ChapterStart::Saved),
        "The first audible target region must begin at the saved four-second marker"
    );
    tokio::task::block_in_place(|| {
        capture::wait(|| {
            let samples = capture.recent(0.1);
            capture::amplitude(&samples, 1320.) > 0.10 && capture::amplitude(&samples, 880.) < 0.03
        })
    });
    assert_tone(&capture.recent(0.1), 1320.);

    let stable = state.borrow().clone();
    let (started, following) = oneshot::channel();
    let (stop, stopped) = oneshot::channel();
    let tracing_position = tokio::spawn(output::trace_position(position.clone(), started, stopped));
    following.await.unwrap();
    let replay_output = capture.mark();
    assert_eq!(player.submit(&f.actor, command).await.unwrap(), receipt);
    let initial_position = position.borrow().position_ms;
    let tempo_output = capture.mark();
    tokio::task::block_in_place(|| capture.after(tempo_output, 0.8));
    let advanced = position
        .borrow()
        .position_ms
        .checked_sub(initial_position)
        .expect("Exact receipt replay must not rewind source position");
    assert!(
        (520..=680).contains(&advanced),
        "Saved .75 tempo must advance source time by about 600ms over 800ms of monitor output; actual={advanced}"
    );
    stop.send(()).unwrap();
    assert!(tracing_position.await.unwrap() >= 5);
    assert_eq!(*state.borrow(), stable);
    let captured = capture.all();
    let replay = &captured[replay_output..tempo_output + 38_400];
    assert_tone(replay, 1320.);
    assert!(
        replay
            .as_chunks::<960>()
            .0
            .iter()
            .all(|window| capture::energy(window) > 0.001),
        "Exact receipt replay must not insert a restart gap"
    );

    player
        .submit(
            &f.actor,
            PlayerCommand {
                id: Uuid::new_v4(),
                lease: attached.lease,
                sequence: 5,
                revision: receipt.revision,
                action: PlayerAction::Pause {},
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        state.wait_for(|state| {
            state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
        }),
    )
    .await
    .unwrap()
    .unwrap();
    hear(&capture, 0.15);
    assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    let paused = position.borrow().position_ms;
    owner.close().await.unwrap();
    let book = f
        .source
        .services
        .collection
        .audiobook(&f.actor, &f.book.to_string())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(book.chapters).unwrap(),
        serde_json::to_value(rendered.chapters).unwrap()
    );
    let continuation = book.continuation.unwrap();
    assert_eq!(continuation.chapter, expected);
    let saved = continuation.progress.unwrap();
    assert_eq!(saved.recording_id, a);
    assert_eq!(saved.position_ms, paused);
    assert_eq!(saved.speed, 0.75);
    assert_eq!(peer_state(&f).await, peer_before);
    for recording in [a, b] {
        assert!(matches!(
            f.source
                .services
                .downloads
                .status(&f.actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(
        std::fs::read(f.source.music.join("track.wav")).unwrap(),
        f.source.original
    );
    assert_eq!(std::fs::read(&f.second_path).unwrap(), f.second_original);
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn first_audible_marker_rejects_both_earlier_and_later_native_start_offsets() {
    let f = audiobook::Library::with_audio(output::write_audio).await;
    let peer_before = peer_state(&f).await;
    let capture = capture::Capture::open();
    let (player, owner) = Player::native(f.source.services.clone());
    let lease = player
        .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
        .await
        .unwrap()
        .lease;
    send(
        &player,
        &f.actor,
        lease,
        1,
        PlayerAction::Replace {
            recordings: vec![f.recordings[0]],
            start: 0,
        },
    )
    .await;
    send(
        &player,
        &f.actor,
        lease,
        2,
        PlayerAction::Speed { rate: 750 },
    )
    .await;
    let mut state = player.subscribe(&f.actor, lease).await.unwrap();
    let mut position = player.subscribe_position(&f.actor, lease).await.unwrap();
    for (index, (offset, expected)) in [
        (3_000, output::ChapterStart::Earlier),
        (5_000, output::ChapterStart::Later),
    ]
    .into_iter()
    .enumerate()
    {
        let sequence = 3 + index as u64 * 3;
        let seek = send(
            &player,
            &f.actor,
            lease,
            sequence,
            PlayerAction::Seek {
                position_ms: offset,
            },
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(3),
            position.wait_for(|position| {
                position.seek_id == Some(seek) && position.position_ms == offset
            }),
        )
        .await
        .unwrap()
        .unwrap();
        hear(&capture, 0.15);
        assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
        let start = capture.mark();
        send(
            &player,
            &f.actor,
            lease,
            sequence + 1,
            PlayerAction::Resume {},
        )
        .await;
        tokio::task::block_in_place(|| {
            capture::wait(|| output::first_chapter_start(&capture.all()[start..]).is_some())
        });
        let observed = output::first_chapter_start(&capture.all()[start..]);
        assert_eq!(observed, Some(expected));
        assert_ne!(
            observed,
            Some(output::ChapterStart::Saved),
            "A wrong native seek must fail the same first-audible saved-marker gate"
        );
        send(
            &player,
            &f.actor,
            lease,
            sequence + 2,
            PlayerAction::Pause {},
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(3),
            state.wait_for(|state| {
                state.phase == PlayerPhase::Ready && !state.playing && !state.desired_playing
            }),
        )
        .await
        .unwrap()
        .unwrap();
        hear(&capture, 0.15);
        assert!(capture::energy(&capture.recent(0.1)) < 0.000001);
    }
    owner.close().await.unwrap();
    assert_eq!(peer_state(&f).await, peer_before);
    assert!(matches!(
        f.source
            .services
            .downloads
            .status(&f.actor, &f.recordings[0].to_string())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        std::fs::read(f.source.music.join("track.wav")).unwrap(),
        f.source.original
    );
}

async fn send(
    player: &Player,
    actor: &hito_core::Actor,
    lease: Uuid,
    sequence: u64,
    action: PlayerAction,
) -> Uuid {
    let id = Uuid::new_v4();
    let revision = player.inspect(actor).await.unwrap().state.unwrap().revision;
    player
        .submit(
            actor,
            PlayerCommand {
                id,
                lease,
                sequence,
                revision,
                action,
            },
        )
        .await
        .unwrap();
    id
}

fn hear(capture: &capture::Capture, seconds: f64) {
    tokio::task::block_in_place(|| capture.after(capture.mark(), seconds));
}

fn assert_tone(samples: &[f32], expected: f64) {
    let amplitude = capture::amplitude(samples, expected);
    assert!(
        amplitude > 0.10,
        "Expected generated {expected}Hz signal must be audible; actual amplitude={amplitude}"
    );
    for frequency in [220., 440., 880., 1320.] {
        if frequency != expected {
            assert!(
                capture::amplitude(samples, frequency) < 0.03,
                "Unexpected generated {frequency}Hz signal was presented"
            );
        }
    }
}

async fn peer_state(f: &audiobook::Library) -> serde_json::Value {
    serde_json::json!({
        "events": f.source.services.collection.events(&f.source.actor, 0).await.unwrap(),
        "activity": f.source.services.collection.activity(&f.source.actor).await.unwrap(),
        "audiobooks": f.source.services.collection.audiobooks(&f.source.actor).await.unwrap(),
    })
}
