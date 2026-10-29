use crate::{AudioFormat, Decoded, Decoder, Result, resample::Decimator};
use std::time::Duration;
use symphonia::core::audio::{Channels, Position};

const CENTER: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Presents a decoded source in the representation the output device path
/// accepts: at most 192 kHz and at most two channels. A source that already has
/// such a representation is forwarded without copying or arithmetic.
pub struct Converter {
    decoder: Decoder,
    output: AudioFormat,
    stage: Option<Stage>,
}

struct Stage {
    source_channels: usize,
    mix: Option<Vec<[f32; 2]>>,
    decimator: Option<Decimator>,
    rate: f64,
    mixed: Vec<f32>,
    samples: Vec<f32>,
    anchor: Duration,
    started: bool,
    finished: bool,
    produced: u64,
}

impl Converter {
    pub fn new(decoder: Decoder) -> Self {
        let source = decoder.format();
        let output = source.output_format();
        let stage = source.requires_conversion().then(|| {
            let mix = (source.channels > 2).then(|| downmix(decoder.layout()));
            let factor = source.decimation() as usize;
            Stage {
                source_channels: usize::from(source.channels),
                mix,
                decimator: (factor > 1)
                    .then(|| Decimator::new(factor, usize::from(output.channels))),
                rate: f64::from(output.sample_rate),
                mixed: Vec::new(),
                samples: Vec::new(),
                anchor: Duration::ZERO,
                started: false,
                finished: false,
                produced: 0,
            }
        });
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
                Some(block) => {
                    if !stage.started {
                        stage.anchor = block.position;
                        stage.started = true;
                    }
                    stage.process(block.samples);
                }
                None => stage.finish(),
            }
        }
        let channels = usize::from(self.output.channels);
        let position = stage.anchor + Duration::from_secs_f64(stage.produced as f64 / stage.rate);
        stage.produced += (stage.samples.len() / channels) as u64;
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
        match &mut self.decimator {
            Some(decimator) => decimator.push(mixed, &mut self.samples),
            None => self.samples.extend_from_slice(mixed),
        }
    }

    fn finish(&mut self) {
        self.finished = true;
        if let Some(decimator) = &mut self.decimator {
            decimator.finish(&mut self.samples);
        }
    }

    fn reset(&mut self) {
        if let Some(decimator) = &mut self.decimator {
            decimator.reset();
        }
        self.started = false;
        self.finished = false;
        self.produced = 0;
        self.samples.clear();
    }
}

/// Stereo weights per source channel, normalised so a full-scale signal in every
/// channel cannot exceed full scale. Low-frequency channels are dropped. A layout
/// without speaker positions alternates channels between left and right.
fn downmix(layout: &Channels) -> Vec<[f32; 2]> {
    let mut weights: Vec<[f32; 2]> = match layout {
        Channels::Positioned(positions) => positions.iter().map(position_weight).collect(),
        Channels::Custom(labels) => labels
            .iter()
            .enumerate()
            .map(|(index, label)| match label {
                symphonia::core::audio::ChannelLabel::Positioned(position) => {
                    position_weight(*position)
                }
                _ => alternate(index),
            })
            .collect(),
        other => (0..other.count()).map(alternate).collect(),
    };
    let peak = (0..2)
        .map(|side| weights.iter().map(|weight| weight[side]).sum::<f32>())
        .fold(0., f32::max);
    if peak > 0. {
        weights
            .iter_mut()
            .flatten()
            .for_each(|weight| *weight /= peak);
    }
    weights
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
