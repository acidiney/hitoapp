#![cfg(feature = "device")]
#[path = "../fixtures/two_tracks.rs"]
mod fixture;
use hito_audio::test_support::capture::{Capture, energy};
use hito_audio::test_support::continuity::{audit, counter, dump, heard, level, longest_run};
use hito_playback::{
    Player, PlayerAction, PlayerCommand, PlayerOwner, PlayerPosition, PlayerSnapshot,
};
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const FIRST: u32 = 24_000;
const SECOND: u32 = 36_000;

struct Run {
    f: fixture::TwoTracks,
    player: Player,
    owner: PlayerOwner,
    lease: Uuid,
    state: watch::Receiver<PlayerSnapshot>,
    position: watch::Receiver<PlayerPosition>,
    sequence: u64,
}

impl Run {
    async fn start(recordings: impl Fn(&fixture::TwoTracks) -> Vec<&str>) -> Self {
        let f = fixture::TwoTracks::start(FIRST, SECOND).await;
        let (player, owner) = Player::native(f.services.clone());
        let lease = player
            .claim(&f.actor, player.inspect(&f.actor).await.unwrap().epoch)
            .await
            .unwrap()
            .lease;
        let state = player.subscribe(&f.actor, lease).await.unwrap();
        let position = player.subscribe_position(&f.actor, lease).await.unwrap();
        let ids = recordings(&f)
            .into_iter()
            .map(|id| Uuid::parse_str(id).unwrap())
            .collect();
        let mut run = Self {
            f,
            player,
            owner,
            lease,
            state,
            position,
            sequence: 0,
        };
        run.send(PlayerAction::Replace {
            recordings: ids,
            start: 0,
        })
        .await;
        run
    }

    async fn send(&mut self, action: PlayerAction) {
        let revision = self.state.borrow().revision;
        self.sequence += 1;
        let receipt = self
            .player
            .submit(
                &self.f.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: self.lease,
                    sequence: self.sequence,
                    revision,
                    action,
                },
            )
            .await
            .unwrap();
        assert_eq!(receipt.sequence, self.sequence);
    }

    async fn entries(&mut self, count: usize) -> Vec<Uuid> {
        let revision = self.state.borrow().revision;
        self.player
            .queue(&self.f.actor, self.lease, revision, 0, count)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.id)
            .collect()
    }

    async fn position_reaches(&mut self, milliseconds: u64) {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.position
                .wait_for(|position| position.position_ms >= milliseconds),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn queue_finishes(&mut self) {
        tokio::time::timeout(
            Duration::from_secs(8),
            self.state.wait_for(|state| state.current.is_none()),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn heard(self, capture: &Capture) -> Vec<f32> {
        self.owner.close().await.unwrap();
        capture.after(capture.mark(), 0.3);
        capture.all()
    }
}

fn pair(f: &fixture::TwoTracks) -> Vec<&str> {
    vec![&f.first.recording, &f.second.recording]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn natural_advance_between_two_recordings_plays_one_continuous_stream() {
    let mut run = Run::start(pair).await;
    let capture = Capture::open();
    let entries = run.entries(2).await;
    run.send(PlayerAction::Resume {}).await;
    let mut position = run.position.clone();
    let boundary = std::sync::Arc::new(std::sync::Mutex::new((0, None)));
    let observed = boundary.clone();
    let watcher = tokio::spawn(async move {
        while position.changed().await.is_ok() {
            let value = *position.borrow_and_update();
            let mut seen = observed.lock().unwrap();
            if value.entry == Some(entries[0]) {
                seen.0 = value.position_ms;
            } else if value.entry == Some(entries[1]) && seen.1.is_none() {
                seen.1 = Some(value.position_ms);
            }
        }
    });
    run.queue_finishes().await;
    let (services, actor) = (run.f.services.clone(), run.f.actor.clone());
    let recordings = (
        run.f.first.recording.clone(),
        run.f.second.recording.clone(),
    );
    let captured = run.heard(&capture).await;
    watcher.abort();
    let (last_first, first_second) = *boundary.lock().unwrap();
    assert!(
        last_first >= 450,
        "first recording stopped reporting at {last_first}"
    );
    assert!(
        first_second.is_some_and(|value| value <= 150),
        "second recording started at {first_second:?}"
    );
    dump("queue", &captured);
    let audit = audit(&captured, counter, (FIRST + SECOND) as usize);
    eprintln!("queue gap frames={}", audit.longest_silence);
    assert_eq!(audit.longest_silence, 0);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, (FIRST + SECOND) as usize);
    let activity = services.collection.activity(&actor).await.unwrap();
    let position = |id: &str| {
        activity
            .history
            .iter()
            .find(|entry| entry.id == id)
            .unwrap()
            .position_ms
    };
    assert_eq!(
        (position(&recordings.0), position(&recordings.1)),
        (500, 750)
    );
}

fn repeat_first(frame: u64) -> i16 {
    counter(frame % u64::from(FIRST))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn removing_the_prepared_entry_replaces_the_lookahead_and_never_plays_it() {
    let mut run =
        Run::start(|f| vec![&f.first.recording, &f.second.recording, &f.first.recording]).await;
    let capture = Capture::open();
    let entries = run.entries(3).await;
    run.send(PlayerAction::Resume {}).await;
    run.position_reaches(100).await;
    run.send(PlayerAction::Remove { entry: entries[1] }).await;
    run.queue_finishes().await;
    let captured = run.heard(&capture).await;
    let audit = audit(&captured, repeat_first, 2 * FIRST as usize);
    assert_eq!(audit.longest_silence, 0);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * FIRST as usize);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn skipping_with_a_prepared_entry_plays_the_next_recording_exactly_once() {
    let mut run = Run::start(pair).await;
    let capture = Capture::open();
    run.send(PlayerAction::Resume {}).await;
    run.position_reaches(100).await;
    run.send(PlayerAction::Next {}).await;
    run.queue_finishes().await;
    let captured = run.heard(&capture).await;
    let heard = heard(&captured);
    assert!(heard.len() > SECOND as usize + 2_000);
    let tail = &heard[heard.len() - SECOND as usize..];
    for (index, sample) in tail.iter().enumerate() {
        assert!(
            (level(*sample) - i32::from(counter(u64::from(FIRST) + index as u64))).abs() <= 1,
            "second recording diverged at {index}"
        );
    }
    let head = &heard[..heard.len() - SECOND as usize];
    assert!(head.len() < FIRST as usize);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn seeking_inside_the_first_recording_keeps_the_gapless_transition() {
    let mut run = Run::start(pair).await;
    let capture = Capture::open();
    run.send(PlayerAction::Resume {}).await;
    run.position_reaches(100).await;
    run.send(PlayerAction::Seek { position_ms: 400 }).await;
    run.queue_finishes().await;
    let captured = run.heard(&capture).await;
    let heard = heard(&captured);
    let tail = &heard[heard.len() - 4_800 - SECOND as usize..];
    for (index, sample) in tail.iter().enumerate() {
        assert!(
            (level(*sample) - i32::from(counter(19_200 + index as u64))).abs() <= 1,
            "stream diverged at {index}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn pausing_before_the_boundary_resumes_into_the_next_recording_without_loss() {
    let mut run = Run::start(pair).await;
    let capture = Capture::open();
    run.send(PlayerAction::Resume {}).await;
    run.position_reaches(420).await;
    run.send(PlayerAction::Pause {}).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes().await;
    let captured = run.heard(&capture).await;
    let audit = audit(&heard(&captured), counter, (FIRST + SECOND) as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, (FIRST + SECOND) as usize);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_chosen_speed_carries_into_the_next_recording_without_a_gap() {
    let mut run = Run::start(pair).await;
    let capture = Capture::open();
    run.send(PlayerAction::Speed { rate: 2000 }).await;
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes().await;
    let captured = run.heard(&capture).await;
    let start = captured.iter().position(|sample| *sample != 0.).unwrap();
    let boundary = start + FIRST as usize / 2;
    let around = &captured[boundary - 2_400..boundary + 2_400];
    eprintln!("queue speed 2 gap frames={}", longest_run(around));
    assert!(longest_run(around) <= 2);
    let total = heard(&captured).len();
    assert!(
        (29_400..=30_600).contains(&total),
        "unexpected length {total}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn logging_out_while_the_next_recording_is_prepared_never_starts_it() {
    let mut run = Run::start(pair).await;
    let capture = Capture::open();
    run.send(PlayerAction::Resume {}).await;
    run.position_reaches(100).await;
    run.f.services.identity.logout(&run.f.token).await.unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    let captured = run.heard(&capture).await;
    assert!(energy(&captured[captured.len() - 4_800..]) < 0.000001);
    assert!(heard(&captured).len() < FIRST as usize);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn an_unplayable_next_entry_keeps_the_existing_boundary_behaviour() {
    let mut run =
        Run::start(|f| vec![&f.first.recording, &f.second.recording, &f.first.recording]).await;
    std::fs::remove_file(run.f.music.join("second.wav")).unwrap();
    hito_core::test_support::support::scan(&run.f.services, &run.f.actor, &run.f.library).await;
    let capture = Capture::open();
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes().await;
    let captured = run.heard(&capture).await;
    let audit = audit(&heard(&captured), repeat_first, 2 * FIRST as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * FIRST as usize);
}
