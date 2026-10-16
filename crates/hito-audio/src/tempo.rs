use crate::{AudioFormat, Decoder, Error, Result};
use soundtouch::SoundTouch;
use std::time::Duration;

pub(crate) struct Block<'a> {
    pub samples: &'a [f32],
    pub position: Duration,
    pub step: f64,
    pub end: Duration,
}

pub(crate) struct Tempo {
    decoder: Decoder,
    stretch: Option<SoundTouch>,
    speed: f32,
    output: Vec<f32>,
    position: Duration,
    decoded_end: Duration,
    flushed: bool,
}

impl Tempo {
    pub fn new(decoder: Decoder) -> Self {
        let channels = usize::from(decoder.format().channels);
        Self {
            decoder,
            stretch: None,
            speed: 1.,
            output: vec![0.; 4096 * channels],
            position: Duration::ZERO,
            decoded_end: Duration::ZERO,
            flushed: false,
        }
    }

    pub fn format(&self) -> AudioFormat {
        self.decoder.format()
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.decoder.seek(position)?;
        self.position = position;
        self.decoded_end = position;
        self.flushed = false;
        self.stretch = if self.speed == 1. {
            None
        } else {
            let mut stretch = SoundTouch::new();
            stretch
                .set_sample_rate(self.format().sample_rate)
                .set_channels(u32::from(self.format().channels))
                .set_tempo(f64::from(self.speed));
            Some(stretch)
        };
        Ok(())
    }

    pub fn speed(&mut self, speed: f32, position: Duration) -> Result<()> {
        self.speed = speed;
        self.seek(position)
    }

    pub fn end(&self) -> Duration {
        self.decoded_end
    }

    pub fn read_until(
        &mut self,
        mut interrupted: impl FnMut() -> bool,
    ) -> Result<Option<Block<'_>>> {
        let format = self.format();
        let channels = usize::from(format.channels);
        let step = f64::from(self.speed) / f64::from(format.sample_rate);
        let Some(stretch) = &mut self.stretch else {
            let Some(block) = self.decoder.read_until(interrupted)? else {
                return Ok(None);
            };
            self.decoded_end = block.position
                + Duration::from_secs_f64(block.samples.len() as f64 / channels as f64 * step);
            return Ok(Some(Block {
                samples: block.samples,
                position: block.position,
                step,
                end: self.decoded_end,
            }));
        };
        loop {
            if interrupted() {
                return Err(Error::Interrupted);
            }
            let frames = stretch.receive_samples(&mut self.output, 4096);
            if frames > 0 {
                let position = self.position;
                self.position = (position + Duration::from_secs_f64(frames as f64 * step))
                    .min(self.decoded_end);
                return Ok(Some(Block {
                    samples: &self.output[..frames * channels],
                    position,
                    step,
                    end: self.decoded_end,
                }));
            }
            if self.flushed {
                return Ok(None);
            }
            if let Some(block) = self.decoder.read_until(&mut interrupted)? {
                self.decoded_end = block.position
                    + Duration::from_secs_f64(
                        block.samples.len() as f64
                            / channels as f64
                            / f64::from(format.sample_rate),
                    );
                stretch.put_samples(block.samples, block.samples.len() / channels);
            } else {
                stretch.flush();
                self.flushed = true;
            }
        }
    }
}
