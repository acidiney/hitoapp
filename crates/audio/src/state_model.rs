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
        let writer = thread::spawn(move || callback.present(1, 17, Some(2.), true, 1));
        if let Some(observed) = state.try_observation() {
            assert!(
                [(0, 1., false, 0), (17, 2., true, 1)].contains(&observed),
                "mixed presentation: {observed:?}"
            );
        }
        writer.join().unwrap();
        assert_eq!(state.try_observation(), Some((17, 2., true, 1)));
    });
}

#[test]
fn resetting_an_ended_observation_keeps_generation_and_cursor_together() {
    model(|| {
        let state = Arc::new(State::new());
        state.present(1, 3, Some(1.5), true, 1);
        let worker = state.clone();
        let writer = thread::spawn(move || worker.reset(11));
        if let Some(observed) = state.try_observation() {
            assert!(
                [(3, 1.5, true, 1), (11, 1.5, false, 1)].contains(&observed),
                "mixed reset: {observed:?}"
            );
        }
        assert_eq!(writer.join().unwrap(), 2);
        assert_eq!(state.try_observation(), Some((11, 1.5, false, 1)));
    });
}

#[test]
fn a_retiring_callback_and_reset_preserve_a_coherent_observation() {
    model(|| {
        let state = Arc::new(State::new());
        state.present(1, 3, Some(1.5), true, 1);
        let callback = state.clone();
        let worker = state.clone();
        let callback = thread::spawn(move || callback.present(1, 5, Some(2.), true, 2));
        let worker = thread::spawn(move || worker.reset(11));
        if let Some(observed) = state.try_observation() {
            assert!(
                [
                    (3, 1.5, true, 1),
                    (5, 2., true, 2),
                    (11, 1.5, false, 1),
                    (11, 2., false, 2),
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
        assert_eq!(final_state.track, 2);
    });
}
