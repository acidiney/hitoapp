#![cfg(feature = "device")]

use hito_audio::{Player, test_support as support};
use support::{
    capture::{Capture, wait},
    continuity::{audit, counter},
    output_witness::{OutputWitness, WitnessError},
};

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_joined_witness_matches_the_actual_owned_counter_and_monitor() {
    let witness = OutputWitness::begin(96_000, 512).unwrap();
    let capture = Capture::open();
    let mut player = Player::open(support::wave(48_000, 2, 24_000, |frame, _| {
        counter(u64::from(frame))
    }))
    .unwrap();
    assert!(matches!(witness.finish(), Err(WitnessError::ActiveWriters)));
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    player.close().unwrap();
    let report = witness.finish().unwrap();
    assert!(!report.callbacks.is_empty());
    assert_eq!(
        report
            .callbacks
            .iter()
            .map(|block| block.frames)
            .sum::<usize>(),
        report.samples.len()
    );
    let direct = audit(&report.samples, counter, 24_000);
    assert_eq!(direct.heard, 24_000);
    assert_eq!(direct.mismatches, 0);
    assert_eq!(direct.longest_silence, 0);
    assert!(report.callbacks.iter().all(|block| {
        block.owner == 1
            && block.generation == 1
            && block.channels == 2
            && block.sample_rate == 48_000
    }));
    capture.after(capture.mark(), 0.2);
    let monitored = audit(&capture.all(), counter, 24_000);
    assert_eq!(monitored.heard, 24_000);
    assert_eq!(monitored.mismatches, 0);
    assert_eq!(monitored.longest_silence, 0);
}
