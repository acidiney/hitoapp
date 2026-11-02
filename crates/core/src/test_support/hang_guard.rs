use std::{future::Future, time::Duration};

pub const HANG_CEILING: Duration = Duration::from_secs(60);

pub async fn hang_guard<T>(label: &str, future: impl Future<Output = T>) -> T {
    match tokio::time::timeout(HANG_CEILING, future).await {
        Ok(value) => value,
        Err(_) => panic!(
            "{label} did not finish within {} seconds",
            HANG_CEILING.as_secs()
        ),
    }
}
