use super::*;
use loom::{model::Builder, sync::Arc, thread};

fn model(check: impl Fn() + Send + Sync + 'static) {
    let mut builder = Builder::new();
    builder.preemption_bound = Some(2);
    builder.max_branches = 1_000;
    builder.check(check);
}

#[test]
fn presented_position_rate_and_completion_are_one_observation() {
    model(|| {
        let state = Arc::new(State::new());
        let callback = state.clone();
        let writer = thread::spawn(move || callback.present(1, 17, Some(2.), true));
        if let Some(observed) = state.try_observation() {
            assert!(
                [(0, 1., false), (17, 2., true)].contains(&observed),
                "mixed presentation: {observed:?}"
            );
        }
        writer.join().unwrap();
        assert_eq!(state.try_observation(), Some((17, 2., true)));
    });
}

#[test]
fn resetting_an_ended_observation_keeps_generation_and_cursor_together() {
    model(|| {
        let state = Arc::new(State::new());
        state.present(1, 3, Some(1.5), true);
        let worker = state.clone();
        let writer = thread::spawn(move || worker.reset(11));
        if let Some(observed) = state.try_observation() {
            assert!(
                [(3, 1.5, true), (11, 1.5, false)].contains(&observed),
                "mixed reset: {observed:?}"
            );
        }
        assert_eq!(writer.join().unwrap(), 2);
        assert_eq!(state.try_observation(), Some((11, 1.5, false)));
    });
}

#[test]
fn a_retiring_callback_and_reset_preserve_a_coherent_observation() {
    model(|| {
        let state = Arc::new(State::new());
        state.present(1, 3, Some(1.5), true);
        let callback = state.clone();
        let worker = state.clone();
        let callback = thread::spawn(move || callback.present(1, 5, Some(2.), true));
        let worker = thread::spawn(move || worker.reset(11));
        if let Some(observed) = state.try_observation() {
            assert!(
                [
                    (3, 1.5, true),
                    (5, 2., true),
                    (11, 1.5, false),
                    (11, 2., false),
                ]
                .contains(&observed),
                "mixed callback/reset: {observed:?}"
            );
        }
        callback.join().unwrap();
        assert_eq!(worker.join().unwrap(), 2);
        let final_state = state.snapshot(AudioFormat {
            sample_rate: 48_000,
            channels: 1,
            duration: None,
        });
        assert_eq!(final_state.position, Duration::from_nanos(11));
        assert_eq!(final_state.speed, 2.);
        assert!(!final_state.ended);
    });
}
