#![cfg(feature = "device")]
#[path = "../fixtures/deferred_library.rs"]
mod fixture;
use fixture::{Deferred, Mode, RATE, Run};
use hito_audio::test_support::continuity::{audit, counter, dump, heard, longest_run};
use hito_playback::PlayerAction;
use std::time::Duration;

fn seconds(count: u32) -> u32 {
    count * RATE
}

fn pair(f: &Deferred) -> Vec<&str> {
    vec![&f.first, &f.second]
}

fn first_twice(f: &Deferred) -> Vec<&str> {
    vec![&f.first, &f.second, &f.first]
}

fn repeat_four(frame: u64) -> i16 {
    counter(frame % u64::from(seconds(4)))
}

fn repeat_six(frame: u64) -> i16 {
    counter(frame % u64::from(seconds(6)))
}

fn report(case: &str, captured: &[f32]) -> usize {
    let start = captured.iter().position(|sample| *sample != 0.).unwrap();
    let end = captured.iter().rposition(|sample| *sample != 0.).unwrap();
    let gap = longest_run(&captured[start..=end]);
    eprintln!(
        "deferred {case} gap frames={gap} ms={}",
        gap * 1000 / RATE as usize
    );
    gap
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_source_ready_before_the_boundary_joins_without_a_gap() {
    let f = Deferred::start(seconds(12), seconds(4), Mode::Ready(Duration::ZERO)).await;
    let total = (seconds(12) + seconds(4)) as usize;
    let mut run = Run::start(f, true, pair).await;
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes(40).await;
    let (captured, events, f) = run.heard().await;
    dump("deferred-ready-before", &captured);
    let gap = report("ready-before-boundary", &captured);
    let audit = audit(&captured, counter, total);
    assert_eq!(gap, 0);
    assert_eq!(audit.longest_silence, 0);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, total);
    assert_eq!(events.count("player_boundary_wait_started"), 0);
    assert_eq!(events.count("player_lookahead_adopted"), 1);
    assert_eq!(f.provider.begins(), 1);
    assert_eq!(f.download_rows().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_source_ready_during_the_boundary_wait_opens_after_a_bounded_gap() {
    let f = Deferred::start(seconds(6), seconds(3), Mode::Ready(Duration::from_secs(8))).await;
    let total = (seconds(6) + seconds(3)) as usize;
    let mut run = Run::start(f, true, pair).await;
    let phases = run.phases();
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes(40).await;
    let (captured, events, f) = run.heard().await;
    dump("deferred-ready-during", &captured);
    let gap = report("ready-during-wait", &captured);
    let audit = audit(&heard(&captured), counter, total);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, total);
    assert!(
        (seconds(1) as usize..=seconds(10) as usize + 24_000).contains(&gap),
        "gap of {gap} frames"
    );
    assert_eq!(events.count("player_boundary_wait_started"), 1);
    assert!(
        events
            .text()
            .contains("\"event\":\"player_boundary_wait_finished\",\"reason\":\"settled\"")
    );
    assert!(
        phases
            .lock()
            .unwrap()
            .iter()
            .any(|phase| phase == "Preparing")
    );
    assert_eq!(f.provider.begins(), 1);
    assert_eq!(f.download_rows().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_source_that_never_becomes_ready_falls_back_after_the_boundary_wait() {
    let f = Deferred::start(seconds(4), seconds(3), Mode::Never).await;
    let mut run = Run::start(f, true, first_twice).await;
    let phases = run.phases();
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes(60).await;
    let (captured, events, f) = run.heard().await;
    dump("deferred-never", &captured);
    let gap = report("never-ready", &captured);
    let audit = audit(&heard(&captured), repeat_four, 2 * seconds(4) as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * seconds(4) as usize);
    assert!(
        (seconds(10) as usize..=seconds(11) as usize + 24_000).contains(&gap),
        "gap of {gap} frames"
    );
    assert!(
        events
            .text()
            .contains("\"event\":\"player_boundary_wait_finished\",\"reason\":\"timeout\"")
    );
    assert!(
        phases
            .lock()
            .unwrap()
            .iter()
            .any(|phase| phase == "Preparing")
    );
    assert_eq!(f.provider.begins(), 1);
    assert_eq!(f.download_rows().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_failed_preparation_ends_the_boundary_hold_at_its_next_status_check() {
    let f = Deferred::start(seconds(4), seconds(3), Mode::Failed).await;
    let mut run = Run::start(f, true, first_twice).await;
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes(40).await;
    let (captured, events, _f) = run.heard().await;
    let gap = report("failed", &captured);
    let audit = audit(&heard(&captured), repeat_four, 2 * seconds(4) as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * seconds(4) as usize);
    assert!(gap < seconds(3) as usize, "gap of {gap} frames");
    assert_eq!(events.count("player_boundary_wait_started"), 1);
    assert!(
        events
            .text()
            .contains("\"event\":\"player_boundary_wait_finished\",\"reason\":\"settled\"")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_saturated_preparation_queue_is_not_admitted_and_never_holds_the_boundary() {
    let f = Deferred::start(seconds(4), seconds(3), Mode::Ready(Duration::ZERO)).await;
    f.saturate_preparations(16).await;
    let rows = f.count("source_preparations").await;
    let mut run = Run::start(f, false, first_twice).await;
    run.send(PlayerAction::Resume {}).await;
    run.queue_finishes(40).await;
    let (captured, events, f) = run.heard().await;
    let gap = report("saturated", &captured);
    let audit = audit(&heard(&captured), repeat_four, 2 * seconds(4) as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * seconds(4) as usize);
    assert!(gap < seconds(3) as usize, "gap of {gap} frames");
    assert_eq!(events.count("player_deferred_source_rejected"), 1);
    assert_eq!(events.count("player_boundary_wait_started"), 0);
    assert_eq!(f.count("source_preparations").await, rows);
    assert_eq!(f.provider.begins(), 0);
    assert_eq!(f.download_rows().await, 0);
}

async fn preparing(run: &Run) {
    run.until("the preparation to be requested", || {
        run.events.count("player_deferred_source_requested") == 1
    })
    .await;
}

async fn leaves_nothing_behind(events: &fixture::Events, f: &Deferred) {
    let checks = events.count("player_deferred_source_checked");
    let begins = f.provider.begins();
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(events.count("player_deferred_source_checked"), checks);
    assert_eq!(f.provider.begins(), begins);
    assert_eq!(f.download_rows().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn skipping_during_preparation_cancels_it_and_leaves_no_work() {
    let f = Deferred::start(seconds(30), seconds(3), Mode::Never).await;
    let mut run = Run::start(f, true, first_twice).await;
    run.send(PlayerAction::Resume {}).await;
    preparing(&run).await;
    run.send(PlayerAction::Next {}).await;
    run.until("the cancellation", || {
        run.events.count("player_deferral_cancelled") >= 1
    })
    .await;
    run.send(PlayerAction::Pause {}).await;
    let (events, f) = {
        let events = run.events.clone();
        let (_, _, f) = run.heard().await;
        (events, f)
    };
    leaves_nothing_behind(&events, &f).await;
    assert_eq!(events.count("player_boundary_wait_started"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn removing_the_preparing_entry_cancels_it_and_leaves_no_work() {
    let f = Deferred::start(seconds(6), seconds(3), Mode::Never).await;
    let mut run = Run::start(f, true, first_twice).await;
    let entries = run.entries(3).await;
    run.send(PlayerAction::Resume {}).await;
    preparing(&run).await;
    run.send(PlayerAction::Remove { entry: entries[1] }).await;
    run.until("the cancellation", || {
        run.events.count("player_deferral_cancelled") >= 1
    })
    .await;
    run.queue_finishes(40).await;
    let (captured, events, f) = run.heard().await;
    let gap = report("removed", &captured);
    let audit = audit(&captured, repeat_six, 2 * seconds(6) as usize);
    assert_eq!(audit.mismatches, 0);
    assert_eq!(audit.heard, 2 * seconds(6) as usize);
    assert_eq!(gap, 0);
    assert_eq!(events.count("player_boundary_wait_started"), 0);
    leaves_nothing_behind(&events, &f).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn seeking_out_of_the_window_cancels_the_preparation() {
    let f = Deferred::start(seconds(200), seconds(3), Mode::Never).await;
    let mut run = Run::start(f, true, pair).await;
    run.send(PlayerAction::Seek {
        position_ms: 160_000,
    })
    .await;
    run.send(PlayerAction::Resume {}).await;
    preparing(&run).await;
    run.send(PlayerAction::Seek { position_ms: 0 }).await;
    run.until("the cancellation", || {
        run.events.count("player_deferral_cancelled") >= 1
    })
    .await;
    run.send(PlayerAction::Pause {}).await;
    let events = run.events.clone();
    let (_, _, f) = run.heard().await;
    leaves_nothing_behind(&events, &f).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn logging_out_during_preparation_cancels_it_and_leaves_no_work() {
    let f = Deferred::start(seconds(30), seconds(3), Mode::Never).await;
    let token = f.token.clone();
    let services = f.services.clone();
    let mut run = Run::start(f, true, pair).await;
    run.send(PlayerAction::Resume {}).await;
    preparing(&run).await;
    services.identity.logout(&token).await.unwrap();
    run.until("the cancellation", || {
        run.events.count("player_deferral_cancelled") >= 1
    })
    .await;
    let events = run.events.clone();
    let (_, _, f) = run.heard().await;
    leaves_nothing_behind(&events, &f).await;
}
