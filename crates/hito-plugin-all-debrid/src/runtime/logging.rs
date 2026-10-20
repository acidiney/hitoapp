use tracing_subscriber::prelude::*;

pub(super) fn initialize() -> Result<(), ()> {
    let level = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            let target = metadata.target();
            [
                "hito_plugin_all_debrid",
                "hito_debrid_all_debrid",
                "hito_remote_audio",
            ]
            .iter()
            .any(|prefix| {
                target == *prefix
                    || target
                        .strip_prefix(*prefix)
                        .is_some_and(|suffix| suffix.starts_with("::"))
            })
        }))
        .with(level)
        .with(tracing_subscriber::fmt::layer().json().with_ansi(false))
        .try_init()
        .map_err(|_| ())
}
