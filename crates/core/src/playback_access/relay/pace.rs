use crate::{Error, Result};
use std::{collections::VecDeque, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamPace {
    pub read_idle: Duration,
    pub window: Duration,
    pub floor_bytes_per_second: u64,
}

impl StreamPace {
    pub const DEFAULT: Self = Self {
        read_idle: READ_IDLE,
        window: WINDOW,
        floor_bytes_per_second: FLOOR_BYTES_PER_SECOND,
    };
}

impl Default for StreamPace {
    fn default() -> Self {
        Self::DEFAULT
    }
}

pub const READ_IDLE: Duration = Duration::from_secs(15);
pub const WINDOW: Duration = Duration::from_secs(30);
pub const FLOOR_BYTES_PER_SECOND: u64 = 4 * 1024;

pub(crate) struct Meter {
    pace: StreamPace,
    samples: VecDeque<(Duration, u64)>,
    waited: Duration,
    bytes: u64,
}

impl Meter {
    pub(crate) fn new(pace: StreamPace) -> Self {
        Self {
            pace,
            samples: VecDeque::new(),
            waited: Duration::ZERO,
            bytes: 0,
        }
    }

    pub(crate) fn read_idle(&self) -> Duration {
        self.pace.read_idle
    }

    pub(crate) fn record(&mut self, waited: Duration, bytes: usize) -> Result<()> {
        let bytes = bytes as u64;
        self.samples.push_back((waited, bytes));
        self.waited += waited;
        self.bytes = self.bytes.saturating_add(bytes);
        while let Some(&(oldest, oldest_bytes)) = self.samples.front() {
            if self.waited - oldest < self.pace.window {
                break;
            }
            self.samples.pop_front();
            self.waited -= oldest;
            self.bytes -= oldest_bytes;
        }
        if self.waited < self.pace.window {
            return Ok(());
        }
        let required = u128::from(self.pace.floor_bytes_per_second) * self.waited.as_nanos();
        if u128::from(self.bytes) * 1_000_000_000 < required {
            return Err(Error::StreamTooSlow);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pace() -> StreamPace {
        StreamPace {
            read_idle: Duration::from_secs(15),
            window: Duration::from_secs(10),
            floor_bytes_per_second: 1000,
        }
    }

    #[test]
    fn a_stream_is_never_judged_before_one_window_of_waiting() {
        let mut meter = Meter::new(pace());
        assert!(meter.record(Duration::from_secs(9), 1).is_ok());
    }

    #[test]
    fn a_trickle_is_cut_off_once_a_window_of_waiting_has_passed() {
        let mut meter = Meter::new(pace());
        assert!(meter.record(Duration::from_secs(5), 10).is_ok());
        assert!(matches!(
            meter.record(Duration::from_secs(5), 10),
            Err(Error::StreamTooSlow)
        ));
    }

    #[test]
    fn a_stream_at_the_floor_passes() {
        let mut meter = Meter::new(pace());
        for _ in 0..20 {
            assert!(meter.record(Duration::from_secs(1), 1000).is_ok());
        }
    }

    #[test]
    fn an_early_burst_ages_out_of_the_window() {
        let mut meter = Meter::new(pace());
        assert!(meter.record(Duration::from_secs(1), 100_000).is_ok());
        for _ in 0..9 {
            assert!(meter.record(Duration::from_secs(1), 10).is_ok());
        }
        assert!(matches!(
            meter.record(Duration::from_secs(1), 10),
            Err(Error::StreamTooSlow)
        ));
    }

    #[test]
    fn time_spent_by_the_consumer_is_not_counted() {
        let mut meter = Meter::new(pace());
        for _ in 0..50 {
            assert!(meter.record(Duration::from_millis(1), 4096).is_ok());
        }
    }
}
