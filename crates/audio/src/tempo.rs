use crate::{AudioFormat, Converter, Error, Result, processor::Processor};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

pub(crate) struct Block<'a> {
    pub samples: &'a [f32],
    pub position: Duration,
    pub step: f64,
    pub end: Duration,
    pub speed: f32,
    pub track: u64,
}

struct Shared {
    next: Mutex<Option<Converter>>,
    ended: AtomicBool,
    speed: AtomicU32,
}

#[derive(Clone)]
pub(crate) struct Handoff(Arc<Shared>);

impl Handoff {
    pub fn set(&self, next: Converter) -> Result<bool> {
        if self.0.ended.load(Ordering::Acquire) {
            return Err(Error::Interrupted);
        }
        let speed = f32::from_bits(self.0.speed.load(Ordering::Acquire));
        if !next.source().speed_capability().supports(speed) {
            return Err(Error::Unsupported);
        }
        Ok(self.0.next.lock().unwrap().replace(next).is_some())
    }

    pub fn announce(&self, speed: f32) {
        self.0.speed.store(speed.to_bits(), Ordering::Release);
    }

    pub fn clear(&self) -> bool {
        self.0.next.lock().unwrap().take().is_some()
    }
}

struct Join {
    out_frames: u64,
    ends_at: Duration,
}

pub(crate) struct Tempo {
    decoder: Converter,
    stretch: Option<Processor>,
    speed: f32,
    requested: Option<f32>,
    input: Vec<f32>,
    input_offset: usize,
    input_position: Duration,
    output: Vec<f32>,
    position: Duration,
    consumed: Duration,
    flushed: bool,
    handoff: Handoff,
    joins: VecDeque<Join>,
    fed: u64,
    emitted: u64,
    track: u64,
}

impl Tempo {
    pub fn new(decoder: Converter) -> Self {
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
            handoff: Handoff(Arc::new(Shared {
                next: Mutex::new(None),
                ended: AtomicBool::new(false),
                speed: AtomicU32::new(1_f32.to_bits()),
            })),
            joins: VecDeque::new(),
            fed: 0,
            emitted: 0,
            track: 0,
        }
    }

    pub fn handoff(&self) -> Handoff {
        self.handoff.clone()
    }

    pub fn track(&self) -> u64 {
        self.track
    }

    pub fn joining(&self) -> bool {
        !self.joins.is_empty()
    }

    fn join(&mut self, next: Converter) {
        self.decoder = next;
        if self.stretch.is_some() {
            self.joins.push_back(Join {
                out_frames: (self.fed as f64 / f64::from(self.speed)).round() as u64,
                ends_at: self.consumed,
            });
        } else {
            self.track += 1;
            self.position = Duration::ZERO;
        }
        self.consumed = Duration::ZERO;
        tracing::debug!(
            event = "audio_tempo_joined",
            track = self.track,
            pending = self.joins.len(),
            "Tempo continued into the next input"
        );
    }

    fn cross(&mut self) {
        self.joins.pop_front();
        self.track += 1;
        self.position = Duration::ZERO;
    }

    pub fn format(&self) -> AudioFormat {
        self.decoder.format()
    }

    fn configure(&mut self, speed: f32) -> Result<()> {
        tracing::debug!(
            event = "audio_tempo_segment_started",
            speed,
            "Audio tempo segment configured"
        );
        self.speed = speed;
        self.position = self.consumed;
        self.flushed = false;
        self.fed = 0;
        self.emitted = 0;
        self.joins.clear();
        self.stretch = if speed == 1. {
            None
        } else {
            Some(Processor::new(self.format(), speed)?)
        };
        Ok(())
    }

    pub fn seek(&mut self, position: Duration) -> Result<()> {
        self.decoder.seek(position)?;
        self.input.clear();
        self.input_offset = 0;
        self.consumed = position;
        self.handoff.0.ended.store(false, Ordering::Release);
        let speed = self.requested.take().unwrap_or(self.speed);
        self.configure(speed)
    }

    pub fn speed(&mut self, speed: f32) {
        self.handoff
            .0
            .speed
            .store(speed.to_bits(), Ordering::Release);
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
                    stretch.flush()?;
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
            if self.stretch.is_some() {
                let limit = self
                    .joins
                    .front()
                    .map(|join| join.out_frames.saturating_sub(self.emitted));
                if limit == Some(0) {
                    self.cross();
                    continue;
                }
                let capacity = limit.map_or(self.output.len() / channels, |limit| {
                    (self.output.len() / channels).min(limit.min(usize::MAX as u64) as usize)
                });
                let frames = self
                    .stretch
                    .as_mut()
                    .unwrap()
                    .receive(&mut self.output[..capacity * channels])?;
                if frames > 0 {
                    let position = self.position;
                    let end = self
                        .joins
                        .front()
                        .map_or(self.consumed, |join| join.ends_at);
                    self.position =
                        (position + Duration::from_secs_f64(frames as f64 * step)).min(end);
                    self.emitted += frames as u64;
                    return Ok(Some(Block {
                        samples: &self.output[..frames * channels],
                        position,
                        step,
                        end,
                        speed: self.speed,
                        track: self.track,
                    }));
                }
            }
            if let Some(speed) = self.requested.take() {
                self.configure(speed)?;
                continue;
            }
            if self.flushed {
                return Ok(None);
            }
            if self.input_offset == self.input.len() {
                let Some(block) = self.decoder.read_until(&mut interrupted)? else {
                    let next = self.handoff.0.next.lock().unwrap().take();
                    if let Some(next) = next {
                        self.join(next);
                        continue;
                    }
                    self.handoff.0.ended.store(true, Ordering::Release);
                    if let Some(stretch) = &mut self.stretch {
                        stretch.flush()?;
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
                stretch.put(&self.input[start..self.input_offset])?;
                self.fed += ((self.input_offset - start) / channels) as u64;
            } else {
                self.position = self.consumed;
                return Ok(Some(Block {
                    samples: &self.input[start..self.input_offset],
                    position: self.input_position
                        + Duration::from_secs_f64(start as f64 / channels as f64 * step),
                    step,
                    end: self.consumed,
                    speed: self.speed,
                    track: self.track,
                }));
            }
        }
    }
}

#[cfg(test)]
#[path = "tempo_join_tests.rs"]
mod join_tests;
