use crate::{render::Frame, state::State};
use std::sync::atomic::Ordering;

pub(crate) struct Buffering {
    capacity: usize,
    epoch: u64,
    frames: usize,
}

impl Buffering {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            epoch: 0,
            frames: 0,
        }
    }

    pub fn queued(&mut self, frame: Frame, state: &State) {
        let epoch = frame.epoch();
        if epoch != state.epoch.load(Ordering::Acquire)
            || state.ready_epoch.load(Ordering::Acquire) == epoch
        {
            return;
        }
        if self.epoch != epoch {
            self.epoch = epoch;
            self.frames = 0;
        }
        let reason = match frame {
            Frame::Audio { .. } => {
                self.frames += 1;
                if self.frames < self.capacity {
                    return;
                }
                "full"
            }
            Frame::End { .. } => "eof",
        };
        state.ready_epoch.store(epoch, Ordering::Release);
        tracing::debug!(
            event = "audio_buffer_ready",
            reason,
            frames = self.frames,
            capacity = self.capacity,
            "Current output buffer ready"
        );
    }
}
