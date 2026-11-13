use crate::error::ErrorResponse;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct AuthLimiter {
    attempts: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl AuthLimiter {
    pub fn allow(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut attempts = self
            .attempts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for bucket in attempts.values_mut() {
            while bucket
                .front()
                .is_some_and(|time| now.duration_since(*time) >= Duration::from_secs(60))
            {
                bucket.pop_front();
            }
        }
        attempts.retain(|_, bucket| !bucket.is_empty());
        if attempts.len() >= 1024 && !attempts.contains_key(&ip) {
            return false;
        }
        let bucket = attempts.entry(ip).or_default();
        if bucket.len() >= 10 {
            return false;
        }
        bucket.push_back(now);
        true
    }
}

pub fn response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", "60")],
        Json(ErrorResponse {
            error: "Too many attempts. Try again in a minute.".into(),
            code: None,
            recording_id: None,
            plugin_connection: None,
        }),
    )
        .into_response()
}
