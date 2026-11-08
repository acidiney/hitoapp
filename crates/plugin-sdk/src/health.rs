use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const HEALTH_PATH: &str = "v1/health";
pub const MAX_HEALTH_BYTES: usize = 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Ok,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum HealthReason {
    Network,
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct HealthReport {
    pub state: HealthState,
    pub reason: Option<HealthReason>,
    pub since: Option<u64>,
}

impl HealthReport {
    pub const fn ok() -> Self {
        Self {
            state: HealthState::Ok,
            reason: None,
            since: None,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match (self.state, self.reason, self.since) {
            (HealthState::Ok, None, None) => Ok(()),
            (HealthState::Degraded, Some(_), Some(since)) if since <= MAX_SAFE_INTEGER => Ok(()),
            _ => Err("The health report is inconsistent"),
        }
    }
}

struct Streaks {
    network: VecDeque<Instant>,
    refused: VecDeque<Instant>,
}

pub struct FailureTracker {
    threshold: usize,
    window: Duration,
    streaks: Mutex<Streaks>,
}

impl FailureTracker {
    pub fn new(threshold: usize, window: Duration) -> Self {
        let threshold = threshold.max(1);
        Self {
            threshold,
            window,
            streaks: Mutex::new(Streaks {
                network: VecDeque::with_capacity(threshold),
                refused: VecDeque::with_capacity(threshold),
            }),
        }
    }

    pub fn record_failure(&self, reason: HealthReason) {
        self.record_failure_at(reason, Instant::now());
    }

    pub fn record_failure_at(&self, reason: HealthReason, now: Instant) {
        let mut streaks = self.lock();
        let recent = match reason {
            HealthReason::Network => &mut streaks.network,
            HealthReason::Refused => &mut streaks.refused,
        };
        if recent.len() == self.threshold {
            recent.pop_front();
        }
        recent.push_back(now);
    }

    pub fn record_success(&self) {
        let mut streaks = self.lock();
        streaks.network.clear();
        streaks.refused.clear();
    }

    pub fn report(&self) -> HealthReport {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        self.report_at(Instant::now(), unix)
    }

    pub fn report_at(&self, now: Instant, unix_seconds: u64) -> HealthReport {
        let streaks = self.lock();
        [
            (HealthReason::Network, &streaks.network),
            (HealthReason::Refused, &streaks.refused),
        ]
        .into_iter()
        .find_map(|(reason, recent)| {
            let oldest = *recent.front().filter(|_| recent.len() == self.threshold)?;
            let age = now.saturating_duration_since(oldest);
            (age <= self.window).then(|| HealthReport {
                state: HealthState::Degraded,
                reason: Some(reason),
                since: Some(unix_seconds.saturating_sub(age.as_secs())),
            })
        })
        .unwrap_or(HealthReport::ok())
    }

    pub fn retained(&self) -> usize {
        let streaks = self.lock();
        streaks.network.len() + streaks.refused.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Streaks> {
        self.streaks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}
