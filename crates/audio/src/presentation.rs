use crate::state::State;
use cpal::StreamInstant;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy)]
pub(crate) struct Point {
    pub at: StreamInstant,
    pub epoch: u64,
    pub position: u64,
    pub ended: bool,
    pub speed: Option<f32>,
    pub track: u64,
}

pub(crate) struct Presentation {
    pending: [Option<Point>; 1024],
    head: usize,
    count: usize,
}

impl Presentation {
    pub fn new() -> Self {
        Self {
            pending: [None; 1024],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, mut point: Point) -> bool {
        if self.count == self.pending.len() {
            return false;
        }
        if self.count > 0 {
            let previous = self.pending[(self.head + self.count - 1) % self.pending.len()].unwrap();
            point.at = point.at.max(previous.at);
        }
        self.pending[(self.head + self.count) % self.pending.len()] = Some(point);
        self.count += 1;
        true
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.count = 0;
    }

    pub fn advance(&mut self, now: StreamInstant, state: &State) {
        while self.count > 0 {
            let point = self.pending[self.head].unwrap();
            let epoch = state.epoch.load(Ordering::Acquire);
            if point.epoch == epoch && point.at > now {
                break;
            }
            self.pending[self.head] = None;
            self.head = (self.head + 1) % self.pending.len();
            self.count -= 1;
            if point.epoch == epoch {
                state.present(
                    point.epoch,
                    point.position,
                    point.speed,
                    point.ended,
                    point.track,
                );
            }
        }
    }
}
