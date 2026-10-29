use tracing_subscriber::prelude::*;

#[derive(Debug)]
pub struct LoggingInitError;

/// Installs JSON logging that keeps only events from the listed crate targets.
pub fn init_logging(targets: &[&str]) -> Result<(), LoggingInitError> {
    let level = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let targets = targets
        .iter()
        .map(|target| (*target).to_owned())
        .collect::<Vec<_>>();
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(move |metadata| {
            let target = metadata.target();
            targets.iter().any(|prefix| {
                target == prefix
                    || target
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|suffix| suffix.starts_with("::"))
            })
        }))
        .with(level)
        .with(tracing_subscriber::fmt::layer().json().with_ansi(false))
        .try_init()
        .map_err(|_| LoggingInitError)
}
