use crate::{AudioFormat, Decoder, Error, Result};
use soundtouch::SoundTouch;
use std::time::Duration;

pub(crate) struct Block<'a> {
    pub samples: &'a [f32],
    pub position: Duration,
    pub step: f64,
    pub end: Duration,
    pub speed: f32,
}

pub(crate) struct Tempo {
    decoder: Decoder,
    stretch: Option<SoundTouch>,
    speed: f32,
    requested: Option<f32>,
    input: Vec<f32>,
    input_offset: usize,
    input_position: Duration,
    output: Vec<f32>,
    position: Duration,
    consumed: Duration,
    flushed: bool,
}

impl Tempo {
    pub fn new(decoder: Decoder) -> Self {
        let channels = usize::from(decoder.format().channels);
        Self {
            decoder,
            stretch: None,
            speed: 1.,
            requested: None,
            input: Vec::new(),
            input_offset: 0,
            input_position: Duration::ZERO,
            output: vec![0.; 4096 * channels],
            position: Duration::ZERO,
            consumed: Duration::ZERO,
            flushed: false,
        }
    }

    pub fn format(&self) -> AudioFormat {
        self.decoder.format()
    }

    fn configure(&mut self, speed: f32) {
        tracing::debug!(
            event = "audio_tempo_segment_started",
            speed,
            "Audio tempo segment configured"
        );
        self.speed = speed;
        self.position = self.consumed;
        self.flushed = false;
        self.stretch = if speed == 1. {
            None
        } else {
            let mut stretch = SoundTouch::new();
            stretch
                .set_sample_rate(self.format().sample_rate)
                .set_channels(u32::from(self.format().channels))
                .set_tempo(f64::from(speed));
            Some(stretch)
        };
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.decoder.seek(position)?;
        self.input.clear();
        self.input_offset = 0;
        self.consumed = position;
        let speed = self.requested.take().unwrap_or(self.speed);
        self.configure(speed);
        Ok(())
    }

    pub fn speed(&mut self, speed: f32) {
        self.requested = Some(speed);
    }

    pub fn end(&self) -> Duration {
        self.consumed
    }

    pub fn read_until(
        &mut self,
        mut interrupted: impl FnMut() -> bool,
    ) -> Result<Option<Block<'_>>> {
        let format = self.format();
        let channels = usize::from(format.channels);
        loop {
            if interrupted() {
                return Err(Error::Interrupted);
            }
            if !self.flushed && self.requested.is_some_and(|speed| speed == self.speed) {
                self.requested = None;
            }
            if self.requested.is_some() && !self.flushed {
                if let Some(stretch) = &mut self.stretch {
                    stretch.flush();
                }
                self.flushed = true;
                tracing::debug!(
                    event = "audio_tempo_segment_draining",
                    speed = self.speed,
                    next_speed = self.requested,
                    "Draining the previous tempo segment"
                );
            }
            let step = f64::from(self.speed) / f64::from(format.sample_rate);
            if let Some(stretch) = &mut self.stretch {
                let frames = stretch.receive_samples(&mut self.output, 4096);
                if frames > 0 {
                    let position = self.position;
                    self.position = (position + Duration::from_secs_f64(frames as f64 * step))
                        .min(self.consumed);
                    return Ok(Some(Block {
                        samples: &self.output[..frames * channels],
                        position,
                        step,
                        end: self.consumed,
                        speed: self.speed,
                    }));
                }
            }
            if let Some(speed) = self.requested.take() {
                self.configure(speed);
                continue;
            }
            if self.flushed {
                return Ok(None);
            }
            if self.input_offset == self.input.len() {
                let Some(block) = self.decoder.read_until(&mut interrupted)? else {
                    if let Some(stretch) = &mut self.stretch {
                        stretch.flush();
                    }
                    self.flushed = true;
                    continue;
                };
                self.input.clear();
                self.input.extend_from_slice(block.samples);
                self.input_position = block.position;
                self.input_offset = 0;
            }
            let start = self.input_offset;
            let count =
                ((format.sample_rate / 100) as usize * channels).min(self.input.len() - start);
            self.input_offset += count;
            self.consumed = self.input_position
                + Duration::from_secs_f64(
                    self.input_offset as f64 / channels as f64 / f64::from(format.sample_rate),
                );
            if let Some(stretch) = &mut self.stretch {
                stretch.put_samples(&self.input[start..self.input_offset], count / channels);
            } else {
                self.position = self.consumed;
                return Ok(Some(Block {
                    samples: &self.input[start..self.input_offset],
                    position: self.input_position
                        + Duration::from_secs_f64(start as f64 / channels as f64 * step),
                    step,
                    end: self.consumed,
                    speed: self.speed,
                }));
            }
        }
    }
}
