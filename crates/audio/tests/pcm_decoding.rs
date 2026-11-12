use hito_audio::{AudioFormat, Decoded, Decoder, Error, PcmSource, Result};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

fn source(samples: Vec<f32>) -> Samples {
    Samples {
        samples,
        position: Duration::ZERO,
        emitted: false,
        format: AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            duration: Some(Duration::from_secs(10)),
        },
    }
}

struct Samples {
    samples: Vec<f32>,
    position: Duration,
    emitted: bool,
    format: AudioFormat,
}

impl PcmSource for Samples {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read(&mut self, _: &mut dyn FnMut() -> bool) -> Result<Option<Decoded<'_>>> {
        if self.emitted {
            return Ok(None);
        }
        self.emitted = true;
        Ok(Some(Decoded {
            samples: &self.samples,
            position: self.position,
        }))
    }

    fn seek(&mut self, position: Duration) -> Result<()> {
        self.position = position;
        self.emitted = false;
        Ok(())
    }
}

#[test]
fn pcm_preserves_exact_samples_format_and_absolute_position() {
    let source = Samples {
        samples: vec![0.25, -0.5, 0.75, -1.],
        position: Duration::from_secs(3),
        emitted: false,
        format: AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            duration: Some(Duration::from_secs(10)),
        },
    };
    let mut decoder = Decoder::from_pcm(source).unwrap();
    assert_eq!(
        decoder.format(),
        AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            duration: Some(Duration::from_secs(10))
        }
    );
    let block = decoder.read().unwrap().unwrap();
    assert_eq!(block.samples, &[0.25, -0.5, 0.75, -1.]);
    assert_eq!(block.position, Duration::from_secs(3));
    assert!(decoder.read().unwrap().is_none());
}

#[test]
fn pcm_rejects_formats_outside_the_qualified_rate_and_channel_bounds() {
    for (sample_rate, channels) in [
        (0, 2),
        (7_999, 1),
        (192_001, 2),
        (384_000, 2),
        (48_000, 0),
        (48_000, 3),
    ] {
        let source = Samples {
            samples: vec![0.; 2],
            position: Duration::ZERO,
            emitted: false,
            format: AudioFormat {
                sample_rate,
                channels,
                duration: None,
            },
        };
        assert!(
            matches!(Decoder::from_pcm(source), Err(Error::Unsupported)),
            "accepted {sample_rate}Hz with {channels} channels"
        );
    }
}

#[test]
fn pcm_rejects_invalid_blocks_before_delivering_samples() {
    for samples in [
        vec![],
        vec![0.; 3],
        vec![f32::NAN, 0.],
        vec![f32::INFINITY, 0.],
        vec![f32::NEG_INFINITY, 0.],
        vec![0.; 1_048_578],
    ] {
        let mut decoder = Decoder::from_pcm(source(samples)).unwrap();
        assert!(matches!(decoder.read(), Err(Error::Decode)));
    }
    let mut overflowing = source(vec![0.; 2]);
    overflowing.position = Duration::MAX;
    let mut decoder = Decoder::from_pcm(overflowing).unwrap();
    assert!(matches!(decoder.read(), Err(Error::Decode)));
}

struct Observed {
    source: Samples,
    reads: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl PcmSource for Observed {
    fn format(&self) -> AudioFormat {
        self.source.format()
    }
    fn read(&mut self, interrupted: &mut dyn FnMut() -> bool) -> Result<Option<Decoded<'_>>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.source.read(interrupted)
    }
    fn seek(&mut self, position: Duration) -> Result<()> {
        self.source.seek(position)
    }
}

impl Drop for Observed {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pcm_interrupted_before_read_never_enters_the_provider() {
    let reads = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let source = Observed {
        source: source(vec![0.; 2]),
        reads: reads.clone(),
        drops: drops.clone(),
    };
    let mut decoder = Decoder::from_pcm(source).unwrap();
    assert!(matches!(
        decoder.read_until(|| true),
        Err(Error::Interrupted)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(decoder);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn pcm_keeps_eof_stable_and_seeks_back_to_absolute_positions() {
    let reads = Arc::new(AtomicUsize::new(0));
    let source = Observed {
        source: source(vec![0.25, -0.5]),
        reads: reads.clone(),
        drops: Arc::new(AtomicUsize::new(0)),
    };
    let mut decoder = Decoder::from_pcm(source).unwrap();
    assert!(decoder.read().unwrap().is_some());
    assert!(decoder.read().unwrap().is_none());
    assert!(decoder.read().unwrap().is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    decoder.seek(Duration::from_secs(7)).unwrap();
    let block = decoder.read().unwrap().unwrap();
    assert_eq!(block.samples, &[0.25, -0.5]);
    assert_eq!(block.position, Duration::from_secs(7));
    assert!(decoder.read().unwrap().is_none());
    decoder.seek(Duration::from_secs(2)).unwrap();
    assert_eq!(
        decoder.read().unwrap().unwrap().position,
        Duration::from_secs(2)
    );
    decoder.seek(Duration::ZERO).unwrap();
    assert_eq!(decoder.read().unwrap().unwrap().position, Duration::ZERO);
    assert_eq!(decoder.format().duration, Some(Duration::from_secs(10)));
}

#[test]
fn pcm_accepts_boundary_formats_and_keeps_unknown_duration_unknown() {
    for (sample_rate, channels) in [(8_000, 1), (8_000, 2), (192_000, 1), (192_000, 2)] {
        let mut source = source(vec![0.; 1_048_576]);
        source.format = AudioFormat {
            sample_rate,
            channels,
            duration: None,
        };
        source.position = Duration::from_secs(3);
        let mut decoder = Decoder::from_pcm(source).unwrap();
        assert_eq!(decoder.read().unwrap().unwrap().samples.len(), 1_048_576);
        decoder.seek(Duration::ZERO).unwrap();
        assert_eq!(decoder.read().unwrap().unwrap().position, Duration::ZERO);
        assert_eq!(
            decoder.format(),
            AudioFormat {
                sample_rate,
                channels,
                duration: None
            }
        );
    }
}

struct Waiting;

impl PcmSource for Waiting {
    fn format(&self) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channels: 1,
            duration: None,
        }
    }
    fn read(&mut self, interrupted: &mut dyn FnMut() -> bool) -> Result<Option<Decoded<'_>>> {
        for _ in 0..8 {
            if interrupted() {
                return Err(Error::Interrupted);
            }
        }
        Err(Error::Worker)
    }
    fn seek(&mut self, _: Duration) -> Result<()> {
        Ok(())
    }
}

#[test]
fn pcm_forwards_interruption_while_the_provider_waits_for_samples() {
    let mut decoder = Decoder::from_pcm(Waiting).unwrap();
    let mut polls = 0;
    assert!(matches!(
        decoder.read_until(|| {
            polls += 1;
            polls >= 3
        }),
        Err(Error::Interrupted)
    ));
}

#[test]
fn pcm_discards_samples_when_interrupted_during_the_read() {
    let mut decoder = Decoder::from_pcm(source(vec![0.; 2])).unwrap();
    let mut polls = 0;
    assert!(matches!(
        decoder.read_until(|| {
            polls += 1;
            polls >= 2
        }),
        Err(Error::Interrupted)
    ));
}

#[test]
fn pcm_rejected_at_construction_releases_the_provider_once_without_reading() {
    let reads = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut source = source(vec![0.; 2]);
    source.format.channels = 0;
    let source = Observed {
        source,
        reads: reads.clone(),
        drops: drops.clone(),
    };
    assert!(matches!(Decoder::from_pcm(source), Err(Error::Unsupported)));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
