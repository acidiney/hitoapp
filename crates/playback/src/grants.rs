use hito_core::PlaybackSource;
use std::{collections::VecDeque, sync::Mutex};

pub(crate) struct Grants {
    chain: Mutex<VecDeque<(u64, PlaybackSource)>>,
}

impl Grants {
    pub fn new(first: PlaybackSource) -> Self {
        Self {
            chain: Mutex::new(VecDeque::from([(0, first)])),
        }
    }

    pub fn active(&self, track: u64) -> PlaybackSource {
        let mut chain = self.chain.lock().unwrap();
        while chain.len() > 1 && chain[1].0 <= track {
            chain.pop_front();
        }
        chain[0].1.clone()
    }

    pub fn push(&self, grant: PlaybackSource, replaced: bool) {
        let mut chain = self.chain.lock().unwrap();
        let last = match (replaced, chain.len()) {
            (true, length) if length > 1 => chain.pop_back().map_or(0, |(track, _)| track - 1),
            _ => chain.back().map_or(0, |(track, _)| *track),
        };
        chain.push_back((last + 1, grant));
    }

    pub fn withdraw(&self) {
        let mut chain = self.chain.lock().unwrap();
        if chain.len() > 1 {
            chain.pop_back();
        }
    }
}
