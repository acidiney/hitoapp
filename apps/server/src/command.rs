use std::{ffi::OsStr, path::PathBuf, process::ExitCode, time::Instant};

pub async fn run() -> Option<ExitCode> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments.next()?;
    let started = Instant::now();
    let mut database = None;
    let mut expected = None;
    let valid = command == OsStr::new("migrate") && {
        let mut valid = true;
        while let Some(flag) = arguments.next() {
            match flag.to_str() {
                Some("--database") if database.is_none() => {
                    database = arguments.next().map(PathBuf::from);
                    valid &= database.is_some();
                }
                Some("--expect-version") if expected.is_none() => {
                    expected = arguments
                        .next()
                        .and_then(|value| value.to_str()?.parse::<i64>().ok());
                    valid &= expected.is_some();
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        valid
    };
    let result = match (valid, database, expected) {
        (true, Some(database), Some(expected)) => match crate::shutdown_signal() {
            Ok(shutdown) => {
                let migration = hito_core::migrate_existing(database, expected);
                tokio::pin!(migration);
                tokio::select! {
                    result = &mut migration => result,
                    _ = shutdown => migration.await,
                }
            }
            Err(_) => Err(hito_core::Error::Internal(
                "Maintenance signals are unavailable".into(),
            )),
        },
        _ => Err(hito_core::Error::Invalid(
            "Use migrate --database PATH --expect-version VERSION".into(),
        )),
    };
    let elapsed = started.elapsed().as_millis();
    Some(match result {
        Ok(outcome) => {
            println!(
                "{{\"event\":\"database_migration_finished\",\"outcome\":\"complete\",\"previous_version\":{},\"current_version\":{},\"applied_count\":{},\"elapsed_ms\":{elapsed}}}",
                outcome.previous_version, outcome.current_version, outcome.applied_count
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "{{\"event\":\"database_migration_finished\",\"outcome\":\"failed\",\"error_code\":\"{}\",\"elapsed_ms\":{elapsed}}}",
                error.code()
            );
            ExitCode::FAILURE
        }
    })
}
