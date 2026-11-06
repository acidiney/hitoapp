use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::{collections::VecDeque, sync::Mutex, time::Duration};

#[derive(Default)]
pub(super) struct AudioFaults {
    plan: Mutex<Plan>,
}

#[derive(Default)]
struct Plan {
    statuses: VecDeque<StatusCode>,
    first_byte_delay: Option<Duration>,
    trickle: Option<(usize, Duration)>,
    ignore_ranges: bool,
}

impl AudioFaults {
    pub(super) fn script_statuses(&self, statuses: Vec<StatusCode>) {
        self.plan.lock().unwrap().statuses = statuses.into();
    }

    pub(super) fn delay_first_byte(&self, delay: Duration) {
        self.plan.lock().unwrap().first_byte_delay = Some(delay);
    }

    pub(super) fn trickle(&self, chunk: usize, interval: Duration) {
        assert!(chunk > 0);
        self.plan.lock().unwrap().trickle = Some((chunk, interval));
    }

    pub(super) fn ignore_ranges(&self) {
        self.plan.lock().unwrap().ignore_ranges = true;
    }

    pub(super) fn ignores_ranges(&self) -> bool {
        self.plan.lock().unwrap().ignore_ranges
    }

    pub(super) fn next_status(&self) -> Option<StatusCode> {
        self.plan.lock().unwrap().statuses.pop_front()
    }

    pub(super) fn first_byte_delay(&self) -> Option<Duration> {
        self.plan.lock().unwrap().first_byte_delay
    }

    pub(super) fn paced(
        &self,
        status: StatusCode,
        headers: HeaderMap,
        bytes: &[u8],
    ) -> Option<Response> {
        let (chunk, interval) = self.plan.lock().unwrap().trickle?;
        let pieces: VecDeque<Vec<u8>> = bytes.chunks(chunk).map(<[u8]>::to_vec).collect();
        let stream =
            futures_util::stream::unfold((pieces, true), move |(mut pieces, first)| async move {
                let piece = pieces.pop_front()?;
                if !first {
                    tokio::time::sleep(interval).await;
                }
                Some((Ok::<_, std::convert::Infallible>(piece), (pieces, false)))
            });
        Some((status, headers, Body::from_stream(stream)).into_response())
    }
}
