use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const MAX_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct Limiter {
    interval: Duration,
    next_start: Arc<Mutex<Option<Instant>>>,
}

impl Limiter {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_start: Arc::default(),
        }
    }

    pub(crate) async fn reserve(&self) -> Result<(), ()> {
        let start = {
            let mut next = self.next_start.lock().await;
            let now = Instant::now();
            let start = next.map_or(now, |at| at.max(now));
            if start.duration_since(now) > MAX_WAIT {
                return Err(());
            }
            *next = Some(start + self.interval);
            start
        };
        tokio::time::sleep_until(start).await;
        Ok(())
    }

    pub(crate) async fn back_off(&self, duration: Duration) {
        let mut next = self.next_start.lock().await;
        let until = Instant::now() + duration;
        *next = Some(next.map_or(until, |at| at.max(until)));
    }
}

#[cfg(test)]
mod tests {
    use super::Limiter;
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn starts_are_spaced_by_the_interval_and_waiting_is_bounded() {
        let limiter = Limiter::new(Duration::from_secs(1));
        let begin = Instant::now();
        let (first, second, third, fourth) = tokio::join!(
            async {
                let result = limiter.reserve().await;
                (result, begin.elapsed())
            },
            async {
                let result = limiter.reserve().await;
                (result, begin.elapsed())
            },
            async {
                let result = limiter.reserve().await;
                (result, begin.elapsed())
            },
            async {
                let result = limiter.reserve().await;
                (result, begin.elapsed())
            },
        );
        assert_eq!(first, (Ok(()), Duration::from_secs(0)));
        assert_eq!(second, (Ok(()), Duration::from_secs(1)));
        assert_eq!(third, (Ok(()), Duration::from_secs(2)));
        assert_eq!(
            fourth.0,
            Err(()),
            "a start more than two seconds away is refused, not queued"
        );
        limiter.reserve().await.unwrap();
        assert_eq!(begin.elapsed(), Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn a_back_off_refuses_starts_until_it_ends() {
        let limiter = Limiter::new(Duration::from_secs(1));
        limiter.back_off(Duration::from_secs(30)).await;
        assert!(limiter.reserve().await.is_err());
        tokio::time::advance(Duration::from_secs(29)).await;
        limiter.reserve().await.unwrap();
    }
}
