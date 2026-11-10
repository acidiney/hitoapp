use super::*;
use cpal::{OutputStreamTimestamp, StreamInstant};

fn format() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channels: 2,
        duration: None,
    }
}

fn time(nanos: u64) -> OutputCallbackInfo {
    OutputCallbackInfo::new(OutputStreamTimestamp {
        callback: StreamInstant::from_nanos(nanos),
        playback: StreamInstant::from_nanos(nanos + 10_000),
    })
}

#[test]
fn every_sample_and_callback_identity_survive_stream_replacement() {
    let journal = Journal::new(16, 8);
    let first = Arc::new(State::new());
    let second = Arc::new(State::new());
    let mut writer = journal.attach(&first, format()).unwrap();
    writer.record(&[0_f32, 99., 0.25, 98.], &time(100_000));
    writer.record(&[0.5_f32, 97.], &time(200_000));
    drop(writer);
    let mut replacement = journal.attach(&first, format()).unwrap();
    first.xruns.store(1, Ordering::Release);
    first.epoch.store(2, Ordering::Release);
    replacement.record(&[0.75_f32, 96.], &time(300_000));
    drop(replacement);
    let mut other = journal.attach(&second, format()).unwrap();
    other.record(&[1_f32, 95.], &time(400_000));
    drop(other);
    let report = journal.finish().unwrap();
    assert_eq!(report.samples, [0., 0.25, 0.5, 0.75, 1.]);
    let identities: Vec<_> = report
        .callbacks
        .iter()
        .map(|block| {
            (
                block.owner,
                block.generation,
                block.sequence,
                block.offset,
                block.frames,
            )
        })
        .collect();
    assert_eq!(
        identities,
        [
            (1, 1, 1, 0, 2),
            (1, 1, 2, 2, 1),
            (1, 2, 1, 3, 1),
            (2, 1, 1, 4, 1)
        ]
    );
    assert_eq!(report.callbacks[0].zeroes, 1);
    assert_eq!(
        (report.callbacks[0].first, report.callbacks[0].last),
        (0., 0.25)
    );
    assert_eq!(report.callbacks[1].callback_ns, 100_000);
    assert_eq!(report.callbacks[1].playback_ns, 110_000);
    assert_eq!(
        (report.callbacks[2].xruns, report.callbacks[2].epoch),
        (1, 2)
    );
}

#[test]
fn frame_and_callback_limits_fail_instead_of_exporting_truncated_success() {
    for (frames, callbacks, data) in [(2, 2, vec![0.25_f32; 6]), (4, 1, vec![0.25_f32; 2])] {
        let journal = Journal::new(frames, callbacks);
        let state = Arc::new(State::new());
        let mut writer = journal.attach(&state, format()).unwrap();
        writer.record(&data, &time(10));
        writer.record(&data, &time(20));
        drop(writer);
        assert!(matches!(journal.finish(), Err(WitnessError::Overflow)));
    }
}

#[test]
fn active_and_incomplete_writers_cannot_publish_a_successful_report() {
    let journal = Journal::new(8, 2);
    let state = Arc::new(State::new());
    let mut writer = journal.attach(&state, format()).unwrap();
    writer.record(&[0.25_f32, 0.75], &time(10));
    assert!(matches!(journal.finish(), Err(WitnessError::ActiveWriters)));
    drop(writer);
    assert_eq!(journal.finish().unwrap().samples, [0.25]);
    assert!(journal.attach(&state, format()).is_none());
    let partial = Journal::new(8, 2);
    partial.callbacks.store(1, Ordering::Release);
    assert!(matches!(partial.finish(), Err(WitnessError::Incomplete)));
    let malformed = Journal::new(8, 2);
    let mut writer = malformed.attach(&state, format()).unwrap();
    writer.record(&[0.25_f32], &time(10));
    drop(writer);
    assert!(matches!(malformed.finish(), Err(WitnessError::Incomplete)));
}

#[test]
fn concurrent_streams_preserve_complete_blocks_and_per_stream_order() {
    let journal = Journal::new(400, 200);
    let threads: Vec<_> = (1..=2)
        .map(|owner| {
            let state = Arc::new(State::new());
            let mut writer = journal.attach(&state, format()).unwrap();
            std::thread::spawn(move || {
                for sequence in 1..=100 {
                    let value = owner as f32 / 4. + sequence as f32 / 1000.;
                    writer.record(&[value, 0., value, 0.], &time(sequence * 100));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let report = journal.finish().unwrap();
    assert_eq!(report.samples.len(), 400);
    assert_eq!(report.callbacks.len(), 200);
    let mut sequences = [0; 2];
    for block in report.callbacks {
        let index = block.owner as usize - 1;
        sequences[index] += 1;
        assert_eq!(block.sequence, sequences[index]);
        assert_eq!(block.frames, 2);
        assert_eq!(
            &report.samples[block.offset..block.offset + block.frames],
            &[block.owner as f32 / 4. + block.sequence as f32 / 1000.; 2]
        );
    }
    assert_eq!(sequences, [100, 100]);
}

#[test]
fn registry_scope_rejects_overlap_and_old_writers_cannot_contaminate_a_new_scope() {
    use super::super::{OutputWitness, attach};
    for (frames, callbacks) in [(0, 1), (1, 0), (usize::MAX, 1), (1, 8193)] {
        assert!(matches!(
            OutputWitness::begin(frames, callbacks),
            Err(WitnessError::InvalidLimits)
        ));
    }
    let first = OutputWitness::begin(8, 4).unwrap();
    assert!(matches!(
        OutputWitness::begin(8, 4),
        Err(WitnessError::Busy)
    ));
    let state = Arc::new(State::new());
    let mut old = attach(&state, format()).unwrap();
    let old_storage = Arc::downgrade(&old.journal);
    old.record(&[0.25_f32, 0.], &time(10));
    drop(first);
    let next = OutputWitness::begin(8, 4).unwrap();
    let mut current = attach(&state, format()).unwrap();
    old.record(&[0.25_f32, 0.], &time(20));
    current.record(&[0.5_f32, 0.], &time(30));
    drop(old);
    assert!(old_storage.upgrade().is_none());
    drop(current);
    assert_eq!(next.finish().unwrap().samples, [0.5]);
}
