#![cfg(feature = "device")]

use hito_audio::test_support::{
    capture::{Capture, wait},
    continuity::{audit, counter},
    output_witness::OutputWitness,
};
use hito_audio::{AudioFormat, Decoded, Decoder, Error, PcmSource, Player, Result};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct DelayedSecondBlock {
    samples: Vec<f32>,
    cursor: usize,
    delayed: bool,
    delay: Duration,
    waiting: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

impl DelayedSecondBlock {
    fn new(delay: Duration) -> Self {
        let samples = (0..48_000_u64)
            .flat_map(|frame| {
                let value = f32::from(counter(frame)) / 32768.;
                [value, value]
            })
            .collect();
        Self {
            samples,
            cursor: 0,
            delayed: false,
            delay,
            waiting: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl PcmSource for DelayedSecondBlock {
    fn format(&self) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            duration: Some(Duration::from_secs_f64(
                self.samples.len() as f64 / 2. / 48_000.,
            )),
        }
    }

    fn read(&mut self, interrupted: &mut dyn FnMut() -> bool) -> Result<Option<Decoded<'_>>> {
        if self.cursor == self.samples.len() {
            return Ok(None);
        }
        if self.cursor == 2 * 1_152 && !self.delayed {
            self.waiting.store(true, Ordering::Release);
            let until = Instant::now() + self.delay;
            while Instant::now() < until {
                if interrupted() {
                    return Err(Error::Interrupted);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            self.delayed = true;
        }
        let start = self.cursor;
        self.cursor = if start == 0 {
            (2 * 1_152).min(self.samples.len())
        } else {
            self.samples.len()
        };
        Ok(Some(Decoded {
            samples: &self.samples[start..self.cursor],
            position: Duration::from_secs_f64(start as f64 / 2. / 48_000.),
        }))
    }

    fn seek(&mut self, position: Duration) -> Result<()> {
        self.cursor =
            ((position.as_secs_f64() * 48_000.).round() as usize).min(self.samples.len() / 2) * 2;
        self.delayed = false;
        Ok(())
    }
}

impl Drop for DelayedSecondBlock {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_delayed_second_pcm_block_does_not_starve_the_initial_output() {
    let witness = OutputWitness::begin(96_000, 512).unwrap();
    let capture = Capture::open();
    let decoder = Decoder::from_pcm(DelayedSecondBlock::new(Duration::from_millis(80))).unwrap();
    let mut player = Player::open_decoder(decoder).unwrap();
    player.resume().unwrap();
    wait(|| player.snapshot().ended);
    let final_state = player.snapshot();
    player.close().unwrap();
    let report = witness.finish().unwrap();
    let direct = audit(&report.samples, counter, 48_000);
    eprintln!(
        "startup callback gap={}, heard={}, underruns={}, xruns={}",
        direct.longest_silence, direct.heard, final_state.underruns, final_state.xruns
    );
    assert_eq!(direct.longest_silence, 0);
    assert_eq!(direct.heard, 48_000);
    assert_eq!(direct.mismatches, 0);
    capture.after(capture.mark(), 0.2);
    let monitored = audit(&capture.all(), counter, 48_000);
    assert_eq!(monitored.longest_silence, 0);
    assert_eq!(monitored.heard, 48_000);
    assert_eq!(monitored.mismatches, 0);
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn paused_startup_can_stop_and_join_a_held_read_without_emitting_partial_audio() {
    let witness = OutputWitness::begin(96_000, 512).unwrap();
    let input = DelayedSecondBlock::new(Duration::from_secs(30));
    let waiting = input.waiting.clone();
    let dropped = input.dropped.clone();
    let mut player = Player::open_decoder(Decoder::from_pcm(input).unwrap()).unwrap();
    player.resume().unwrap();
    wait(|| waiting.load(Ordering::Acquire));
    player.pause();
    assert!(!player.snapshot().playing);
    assert_eq!(player.snapshot().position, Duration::ZERO);
    let closing = Instant::now();
    player.close().unwrap();
    assert!(closing.elapsed() < Duration::from_millis(500));
    assert!(dropped.load(Ordering::Acquire));
    let report = witness.finish().unwrap();
    assert!(report.samples.iter().all(|sample| *sample == 0.));
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn a_seek_during_initial_buffering_retires_the_old_epoch_without_a_full_ring_deadlock() {
    let witness = OutputWitness::begin(96_000, 512).unwrap();
    let input = DelayedSecondBlock::new(Duration::from_secs(30));
    let waiting = input.waiting.clone();
    let mut player = Player::open_decoder(Decoder::from_pcm(input).unwrap()).unwrap();
    player.resume().unwrap();
    wait(|| waiting.load(Ordering::Acquire));
    player.seek(Duration::from_millis(500)).unwrap();
    wait(|| player.snapshot().ended);
    player.close().unwrap();
    let report = witness.finish().unwrap();
    let expected = |frame| counter(frame + 24_000);
    let direct = audit(&report.samples, expected, 24_000);
    assert_eq!(direct.heard, 24_000);
    assert_eq!(direct.mismatches, 0);
    assert_eq!(direct.longest_silence, 0);
}

#[test]
#[ignore = "requires the isolated clocked audio monitor"]
fn short_and_empty_sources_release_the_gate_on_their_end_marker() {
    for frames in [0, 2_400] {
        let witness = OutputWitness::begin(96_000, 512).unwrap();
        let capture = Capture::open();
        let mut input = DelayedSecondBlock::new(Duration::from_millis(80));
        input.samples.truncate(frames * 2);
        let mut player = Player::open_decoder(Decoder::from_pcm(input).unwrap()).unwrap();
        player.resume().unwrap();
        wait(|| player.snapshot().ended);
        assert_eq!(
            player.snapshot().position,
            Duration::from_secs_f64(frames as f64 / 48_000.)
        );
        player.close().unwrap();
        let report = witness.finish().unwrap();
        capture.after(capture.mark(), 0.2);
        if frames == 0 {
            assert!(report.samples.iter().all(|sample| *sample == 0.));
            assert!(capture.all().iter().all(|sample| *sample == 0.));
        } else {
            for samples in [&report.samples, &capture.all()] {
                let measured = audit(samples, counter, frames);
                assert_eq!(measured.heard, frames);
                assert_eq!(measured.mismatches, 0);
                assert_eq!(measured.longest_silence, 0);
            }
        }
    }
}
