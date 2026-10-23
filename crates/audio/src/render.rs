use crate::{
    AudioFormat, Error,
    presentation::{Point, Presentation},
    state::State,
};
use cpal::{FromSample, OutputCallbackInfo, SizedSample, StreamInstant};
use rtrb::Consumer;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Frame {
    Audio {
        samples: [f32; 2],
        speed: f32,
        position: u64,
        epoch: u64,
    },
    End {
        position: u64,
        epoch: u64,
    },
}

impl Frame {
    fn epoch(self) -> u64 {
        match self {
            Self::Audio { epoch, .. } | Self::End { epoch, .. } => epoch,
        }
    }
}

pub(crate) struct Render {
    consumer: Consumer<Frame>,
    state: Arc<State>,
    format: AudioFormat,
    presentation: Presentation,
    scheduled_end: u64,
    next_buffer_at: Option<StreamInstant>,
    xruns: u64,
}

impl Render {
    pub fn new(consumer: Consumer<Frame>, state: Arc<State>, format: AudioFormat) -> Self {
        Self {
            consumer,
            state,
            format,
            presentation: Presentation::new(),
            scheduled_end: 0,
            next_buffer_at: None,
            xruns: 0,
        }
    }

    fn discard_before(&mut self, epoch: u64, budget: &mut usize) {
        while *budget > 0
            && self
                .consumer
                .peek()
                .is_ok_and(|frame| frame.epoch() < epoch)
        {
            *budget -= 1;
            let _ = self.consumer.pop();
        }
    }

    fn take(&mut self, epoch: u64, budget: &mut usize) -> Option<Frame> {
        self.discard_before(epoch, budget);
        if self
            .consumer
            .peek()
            .is_ok_and(|frame| frame.epoch() == epoch)
        {
            self.consumer.pop().ok()
        } else {
            None
        }
    }

    fn schedule(&mut self, point: Point) {
        if !self.presentation.push(point) {
            self.state.fail(Error::Output);
        }
    }

    pub fn fill<T: SizedSample + FromSample<f32>>(
        &mut self,
        data: &mut [T],
        info: &OutputCallbackInfo,
    ) {
        let timestamp = info.timestamp();
        let xruns = self.state.xruns.load(Ordering::Acquire);
        if xruns != self.xruns {
            self.xruns = xruns;
            self.presentation.clear();
            self.next_buffer_at = None;
        }
        self.presentation.advance(timestamp.callback, &self.state);
        let playback = self
            .next_buffer_at
            .map_or(timestamp.playback.max(timestamp.callback), |previous| {
                previous.max(timestamp.playback).max(timestamp.callback)
            });
        self.next_buffer_at =
            Some(self.at(playback, data.len() / usize::from(self.format.channels)));
        data.fill(T::EQUILIBRIUM);
        let volume = f32::from_bits(self.state.volume.load(Ordering::Relaxed));
        let mut underrun = false;
        let mut budget = (self.format.sample_rate / 10) as usize;
        let mut last = None;
        for (index, output) in data
            .chunks_exact_mut(usize::from(self.format.channels))
            .enumerate()
        {
            let epoch = self.state.epoch.load(Ordering::Acquire);
            if self.state.recovered_xruns.load(Ordering::Acquire) < xruns {
                continue;
            }
            self.discard_before(epoch, &mut budget);
            if !self.state.playing.load(Ordering::Acquire)
                || self.state.stopped.load(Ordering::Acquire)
                || self.state.failure.load(Ordering::Acquire) != 0
            {
                continue;
            }
            let Some(frame) = self.take(epoch, &mut budget) else {
                underrun |= self.scheduled_end != epoch;
                continue;
            };
            if frame.epoch() != self.state.epoch.load(Ordering::Acquire) {
                continue;
            }
            match frame {
                Frame::Audio {
                    samples,
                    speed,
                    position,
                    epoch,
                } => {
                    for (output, sample) in output.iter_mut().zip(samples) {
                        *output = T::from_sample((sample * volume).clamp(-1., 1.));
                    }
                    last = Some(Point {
                        at: self.at(playback, index + 1),
                        epoch,
                        position,
                        ended: false,
                        speed: Some(speed),
                    });
                }
                Frame::End { position, epoch } => {
                    if let Some(point) = last.take() {
                        self.schedule(point);
                    }
                    self.schedule(Point {
                        at: self.at(playback, index),
                        epoch,
                        position,
                        ended: true,
                        speed: None,
                    });
                    self.scheduled_end = epoch;
                }
            }
        }
        if let Some(point) = last {
            self.schedule(point);
        }
        if underrun {
            self.state.underruns.fetch_add(1, Ordering::Relaxed);
        }
        self.state.callbacks.fetch_add(1, Ordering::Release);
    }

    fn at(&self, start: StreamInstant, frames: usize) -> StreamInstant {
        start + Duration::from_secs_f64(frames as f64 / f64::from(self.format.sample_rate))
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
