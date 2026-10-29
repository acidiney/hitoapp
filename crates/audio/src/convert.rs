use crate::{AudioFormat, Decoded, Decoder, Result, resample::Decimator};
use std::time::Duration;
use symphonia::core::audio::{Channels, Position};

const CENTER: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Presents a decoded source in the representation the output device path
/// accepts: at most 192 kHz and at most two channels. A source that already has
/// such a representation is forwarded unchanged.
pub struct Converter {
    decoder: Decoder,
    output: AudioFormat,
    stage: Option<Stage>,
}

struct Stage {
    source_channels: usize,
    output_channels: usize,
    mix: Option<Vec<[f32; 2]>>,
    limiter: Option<Limiter>,
    decimator: Option<Decimator>,
    factor: u64,
    source_rate: f64,
    duration: Option<Duration>,
    mixed: Vec<f32>,
    samples: Vec<f32>,
    segment: Segment,
    consumed: u64,
    produced: u64,
    started: bool,
    finished: bool,
}

/// Maps converted frames to media time. A segment starts at the first packet
/// after a start, a seek, or a gap in packet times.
#[derive(Default, Clone, Copy)]
struct Segment {
    time: Duration,
    consumed: u64,
}

impl Converter {
    pub fn new(decoder: Decoder) -> Self {
        let source = decoder.format();
        let output = source.output_format();
        let stage = source
            .requires_conversion()
            .then(|| Stage::new(source, output, decoder.layout()));
        tracing::debug!(
            event = "audio_conversion_configured",
            source_rate = source.sample_rate,
            source_channels = source.channels,
            output_rate = output.sample_rate,
            output_channels = output.channels,
            converted = stage.is_some(),
            "Audio conversion configured"
        );
        Self {
            decoder,
            output,
            stage,
        }
    }

    /// The converted representation. Its duration is the source duration.
    pub fn format(&self) -> AudioFormat {
        self.output
    }

    pub fn source(&self) -> AudioFormat {
        self.decoder.format()
    }

    pub fn read(&mut self) -> Result<Option<Decoded<'_>>> {
        self.read_until(|| false)
    }

    pub fn read_until(
        &mut self,
        mut interrupted: impl FnMut() -> bool,
    ) -> Result<Option<Decoded<'_>>> {
        let Some(stage) = self.stage.as_mut() else {
            return self.decoder.read_until(interrupted);
        };
        stage.samples.clear();
        while stage.samples.is_empty() {
            if stage.finished {
                return Ok(None);
            }
            match self.decoder.read_until(&mut interrupted)? {
                Some(block) => stage.accept(block.position, block.samples),
                None => stage.finish(),
            }
        }
        let position = stage.emitted();
        Ok(Some(Decoded {
            samples: &stage.samples,
            position,
        }))
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.decoder.seek(position)?;
        if let Some(stage) = &mut self.stage {
            stage.reset();
        }
        Ok(())
    }
}

impl Stage {
    fn new(source: AudioFormat, output: AudioFormat, layout: &Channels) -> Self {
        let factor = source.decimation() as usize;
        let mix = (source.channels > 2).then(|| downmix(layout));
        Stage {
            source_channels: usize::from(source.channels),
            output_channels: usize::from(output.channels),
            limiter: mix.is_some().then(|| Limiter::new(output.sample_rate)),
            mix,
            decimator: (factor > 1).then(|| Decimator::new(factor, usize::from(output.channels))),
            factor: factor as u64,
            source_rate: f64::from(source.sample_rate),
            duration: source.duration,
            mixed: Vec::new(),
            samples: Vec::new(),
            segment: Segment::default(),
            consumed: 0,
            produced: 0,
            started: false,
            finished: false,
        }
    }

    fn accept(&mut self, position: Duration, input: &[f32]) {
        let expected = self.segment.time.as_secs_f64()
            + (self.consumed - self.segment.consumed) as f64 / self.source_rate;
        let tolerance = self.factor as f64 / self.source_rate;
        if !self.started || (position.as_secs_f64() - expected).abs() > tolerance {
            self.segment = Segment {
                time: position,
                consumed: self.consumed,
            };
            self.started = true;
        }
        self.consumed += (input.len() / self.source_channels) as u64;
        self.process(input);
    }

    /// The media position of the samples ready to be read, which are then consumed.
    fn emitted(&mut self) -> Duration {
        let offset = (self.produced * self.factor) as f64 - self.segment.consumed as f64;
        let seconds = self.segment.time.as_secs_f64() + offset / self.source_rate;
        let position = Duration::try_from_secs_f64(seconds.max(0.)).unwrap_or_default();
        self.produced += (self.samples.len() / self.output_channels) as u64;
        self.duration.map_or(position, |end| position.min(end))
    }

    fn process(&mut self, input: &[f32]) {
        let mixed: &[f32] = if let Some(weights) = &self.mix {
            self.mixed.clear();
            for frame in input.chunks_exact(self.source_channels) {
                let (mut left, mut right) = (0., 0.);
                for (sample, weight) in frame.iter().zip(weights) {
                    left += sample * weight[0];
                    right += sample * weight[1];
                }
                self.mixed.extend_from_slice(&[left, right]);
            }
            &self.mixed
        } else {
            input
        };
        let ready = self.samples.len();
        match &mut self.decimator {
            Some(decimator) => decimator.push(mixed, &mut self.samples),
            None => self.samples.extend_from_slice(mixed),
        }
        self.limit(ready);
    }

    fn finish(&mut self) {
        self.finished = true;
        let ready = self.samples.len();
        if let Some(decimator) = &mut self.decimator {
            decimator.finish(&mut self.samples);
        }
        self.limit(ready);
    }

    fn limit(&mut self, from: usize) {
        if let Some(limiter) = &mut self.limiter {
            limiter.apply(&mut self.samples[from..]);
        }
    }

    fn reset(&mut self) {
        if let Some(decimator) = &mut self.decimator {
            decimator.reset();
        }
        if let Some(limiter) = &mut self.limiter {
            limiter.reset();
        }
        self.started = false;
        self.finished = false;
        self.consumed = 0;
        self.produced = 0;
        self.segment = Segment::default();
        self.samples.clear();
    }
}

/// Keeps a downmixed stereo signal at or below full scale. The gain is unity
/// until a frame would exceed full scale, drops at once to keep that frame at
/// full scale, and returns to unity over `RELEASE_SECONDS`.
struct Limiter {
    gain: f32,
    release: f32,
}

const RELEASE_SECONDS: f32 = 0.15;

impl Limiter {
    fn new(rate: u32) -> Self {
        Self {
            gain: 1.,
            release: 1. - (-1. / (RELEASE_SECONDS * rate as f32)).exp(),
        }
    }

    fn reset(&mut self) {
        self.gain = 1.;
    }

    fn apply(&mut self, samples: &mut [f32]) {
        for frame in samples.as_chunks_mut::<2>().0 {
            let peak = frame[0].abs().max(frame[1].abs());
            let mut gain = self.gain + (1. - self.gain) * self.release;
            if 1. - gain < 1e-6 {
                gain = 1.;
            }
            if peak * gain > 1. {
                gain = 1. / peak;
            }
            self.gain = gain;
            if gain < 1. {
                for sample in frame {
                    *sample = (*sample * gain).clamp(-1., 1.);
                }
            }
        }
    }
}

/// Stereo weights per source channel using ITU-R BS.775 coefficients: front
/// channels at unity, centre and surround channels at -3 dB, low-frequency
/// channels dropped. The weights are not normalised, so a front-only signal
/// keeps its level; the limiter keeps loud multichannel mixes below full scale.
/// A layout without speaker positions uses the default speaker order for its
/// channel count, and alternates channels between left and right beyond eight.
fn downmix(layout: &Channels) -> Vec<[f32; 2]> {
    match layout {
        Channels::Positioned(positions) => positions.iter().map(position_weight).collect(),
        Channels::Custom(labels) => labels
            .iter()
            .enumerate()
            .map(|(index, label)| match label {
                symphonia::core::audio::ChannelLabel::Positioned(position) => {
                    position_weight(*position)
                }
                _ => default_weight(labels.len(), index),
            })
            .collect(),
        other => (0..other.count())
            .map(|index| default_weight(other.count(), index))
            .collect(),
    }
}

fn default_weight(count: usize, index: usize) -> [f32; 2] {
    const FL: Position = Position::FRONT_LEFT;
    const FR: Position = Position::FRONT_RIGHT;
    const FC: Position = Position::FRONT_CENTER;
    const LFE: Position = Position::LFE1;
    const RL: Position = Position::REAR_LEFT;
    const RR: Position = Position::REAR_RIGHT;
    const RC: Position = Position::REAR_CENTER;
    const SL: Position = Position::SIDE_LEFT;
    const SR: Position = Position::SIDE_RIGHT;
    let order: &[Position] = match count {
        3 => &[FL, FR, FC],
        4 => &[FL, FR, RL, RR],
        5 => &[FL, FR, FC, RL, RR],
        6 => &[FL, FR, FC, LFE, RL, RR],
        7 => &[FL, FR, FC, LFE, RC, SL, SR],
        8 => &[FL, FR, FC, LFE, RL, RR, SL, SR],
        _ => &[],
    };
    order
        .get(index)
        .map_or_else(|| alternate(index), |position| position_weight(*position))
}

fn alternate(index: usize) -> [f32; 2] {
    if index.is_multiple_of(2) {
        [1., 0.]
    } else {
        [0., 1.]
    }
}

fn position_weight(position: Position) -> [f32; 2] {
    const LEFT: Position = Position::FRONT_LEFT
        .union(Position::FRONT_LEFT_CENTER)
        .union(Position::FRONT_LEFT_WIDE);
    const RIGHT: Position = Position::FRONT_RIGHT
        .union(Position::FRONT_RIGHT_CENTER)
        .union(Position::FRONT_RIGHT_WIDE);
    const SURROUND_LEFT: Position = Position::REAR_LEFT
        .union(Position::SIDE_LEFT)
        .union(Position::TOP_FRONT_LEFT)
        .union(Position::TOP_REAR_LEFT)
        .union(Position::TOP_SIDE_LEFT)
        .union(Position::BOTTOM_FRONT_LEFT);
    const SURROUND_RIGHT: Position = Position::REAR_RIGHT
        .union(Position::SIDE_RIGHT)
        .union(Position::TOP_FRONT_RIGHT)
        .union(Position::TOP_REAR_RIGHT)
        .union(Position::TOP_SIDE_RIGHT)
        .union(Position::BOTTOM_FRONT_RIGHT);
    const LOW_FREQUENCY: Position = Position::LFE1.union(Position::LFE2);
    if position.intersects(LOW_FREQUENCY) {
        [0., 0.]
    } else if position.intersects(LEFT) {
        [1., 0.]
    } else if position.intersects(RIGHT) {
        [0., 1.]
    } else if position.intersects(SURROUND_LEFT) {
        [CENTER, 0.]
    } else if position.intersects(SURROUND_RIGHT) {
        [0., CENTER]
    } else {
        [CENTER, CENTER]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: f32 = CENTER;

    fn stage(rate: u32, channels: u16, duration: Option<Duration>, layout: &Channels) -> Stage {
        let source = AudioFormat {
            sample_rate: rate,
            channels,
            duration,
        };
        Stage::new(source, source.output_format(), layout)
    }

    fn stereo() -> Channels {
        Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT)
    }

    fn feed(stage: &mut Stage, position: Duration, frames: usize) -> Duration {
        stage.samples.clear();
        stage.accept(position, &vec![0.; frames * stage.source_channels]);
        stage.emitted()
    }

    #[test]
    fn output_positions_follow_the_packet_timeline_across_a_gap() {
        let mut stage = stage(384_000, 2, None, &stereo());
        feed(&mut stage, Duration::ZERO, 4_000);
        let resumed = feed(&mut stage, Duration::from_millis(500), 4_000);
        assert!(
            (resumed.as_secs_f64() - 0.5).abs() < 0.001,
            "stream resumed at {resumed:?} instead of the packet time"
        );
        let next = feed(&mut stage, Duration::from_micros(500_000 + 10_416), 4_000);
        assert!(
            (next.as_secs_f64() - 0.5).abs() < 0.02,
            "contiguous packets drifted to {next:?}"
        );
    }

    #[test]
    fn positions_never_pass_the_source_duration() {
        let duration = Duration::from_secs(1);
        let mut stage = stage(384_000, 2, Some(duration), &stereo());
        let position = feed(&mut stage, Duration::from_millis(1_500), 4_000);
        assert!(position <= duration, "{position:?}");
    }

    fn weights(count: u16) -> Vec<[f32; 2]> {
        downmix(&Channels::Discrete(count))
    }

    #[test]
    fn unpositioned_layouts_use_the_default_speaker_order_for_their_count() {
        let (l, r, c, s, none) = ([1., 0.], [0., 1.], [C, C], [C, 0.], [0., 0.]);
        let sr = [0., C];
        assert_eq!(weights(3), [l, r, c]);
        assert_eq!(weights(4), [l, r, s, sr]);
        assert_eq!(weights(5), [l, r, c, s, sr]);
        assert_eq!(weights(6), [l, r, c, none, s, sr]);
        assert_eq!(weights(7), [l, r, c, none, c, s, sr]);
        assert_eq!(weights(8), [l, r, c, none, s, sr, s, sr]);
    }

    #[test]
    fn downmix_keeps_front_channels_at_unity_and_attenuates_others_by_three_decibels() {
        let layout = Channels::Positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::FRONT_CENTER
                | Position::LFE1
                | Position::REAR_LEFT
                | Position::REAR_RIGHT,
        );
        assert_eq!(
            downmix(&layout),
            [[1., 0.], [0., 1.], [C, C], [0., 0.], [C, 0.], [0., C]]
        );
    }
}
