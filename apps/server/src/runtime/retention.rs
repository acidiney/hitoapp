use hito_core::LibraryStorage;
use std::time::{Duration, Instant};
use tokio::sync::watch;

#[derive(Clone, Copy)]
struct Schedule {
    first: Duration,
    regular: Duration,
    more: Duration,
}

const SCHEDULE: Schedule = Schedule {
    first: Duration::from_secs(120),
    regular: Duration::from_secs(6 * 60 * 60),
    more: Duration::from_secs(5),
};

#[tracing::instrument(level = "info", skip_all)]
pub(super) async fn worker(storage: LibraryStorage, stopped: watch::Receiver<bool>) {
    run(storage, stopped, SCHEDULE).await;
}

async fn run(storage: LibraryStorage, mut stopped: watch::Receiver<bool>, schedule: Schedule) {
    tracing::info!(
        event = "retention_worker_started",
        "Missing recording retention worker started"
    );
    let mut wait = schedule.first;
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = tokio::time::sleep(wait) => {}
        }
        let started = Instant::now();
        wait = match storage.purge_expired_missing_recordings().await {
            Ok(outcome) if outcome.more => schedule.more,
            Ok(_) => schedule.regular,
            Err(error) => {
                tracing::error!(
                    event = "retention_pass_failed",
                    error_kind = error.code(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Missing recording retention pass failed"
                );
                schedule.regular
            }
        };
    }
    tracing::info!(
        event = "retention_worker_stopped",
        "Missing recording retention worker stopped"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use hito_core::test_support::support;
    use sqlx::Connection;

    #[tokio::test]
    async fn the_worker_removes_expired_missing_recordings_and_stops_on_request() {
        let (dir, hito, _actor, library, _music) = support::library("retention").await;
        let mut connection = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("hito.db")),
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO recordings(id, library_id, title, artist) VALUES ('expired', ?, 'Expired', 'Nobody')")
            .bind(&library)
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sources(id, recording_id, provider, provider_id, device_id, ready) VALUES ('expired-source', 'expired', 'local', 'expired-source', 'retention', 0)")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("UPDATE source_unavailability SET since = unixepoch() - 40 * 86400")
            .execute(&mut connection)
            .await
            .unwrap();
        let (stop, stopped) = watch::channel(false);
        let worker = tokio::spawn(run(
            hito.storage.clone(),
            stopped,
            Schedule {
                first: Duration::from_millis(10),
                regular: Duration::from_millis(20),
                more: Duration::from_millis(10),
            },
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left: i64 =
                sqlx::query_scalar("SELECT count(*) FROM recordings WHERE id='expired'")
                    .fetch_one(&mut connection)
                    .await
                    .unwrap();
            if left == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the worker never removed the recording"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .expect("the worker stops promptly")
            .unwrap();
    }
}
