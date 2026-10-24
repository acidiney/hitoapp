use super::RuntimeResult;
use hito_core::{FolderWatchSettings, WatchMode};
use std::time::Duration;

const MIN_POLL_SECONDS: u64 = 5;
const MAX_POLL_SECONDS: u64 = 86_400;

pub(super) fn parse_folder_watching(
    mode: Option<&str>,
    poll_seconds: Option<&str>,
) -> RuntimeResult<FolderWatchSettings> {
    let text = |value: Option<&str>| {
        value
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
    };
    let mode = match text(mode).as_deref() {
        None | Some("auto") => WatchMode::Auto,
        Some("poll") => WatchMode::Poll,
        Some("off") => WatchMode::Off,
        Some(_) => {
            return Err(invalid("HITO_FOLDER_WATCH must be auto, poll, or off"));
        }
    };
    let mut settings = FolderWatchSettings {
        mode,
        ..Default::default()
    };
    if mode != WatchMode::Off
        && let Some(value) = text(poll_seconds)
    {
        let seconds = value
            .parse::<u64>()
            .ok()
            .filter(|seconds| (MIN_POLL_SECONDS..=MAX_POLL_SECONDS).contains(seconds))
            .ok_or_else(|| {
                invalid("HITO_FOLDER_WATCH_POLL_SECONDS must be a whole number from 5 to 86400")
            })?;
        settings.poll_interval = Duration::from_secs(seconds);
    }
    Ok(settings)
}

fn invalid(message: &'static str) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_automatic_watching_with_a_slow_poll() {
        let settings = parse_folder_watching(None, None).unwrap();
        assert_eq!(settings.mode, WatchMode::Auto);
        assert_eq!(settings.poll_interval, Duration::from_secs(30));
    }

    #[test]
    fn operators_can_disable_watching_or_force_polling() {
        assert_eq!(
            parse_folder_watching(Some("off"), None).unwrap().mode,
            WatchMode::Off
        );
        let settings = parse_folder_watching(Some("poll"), Some("120")).unwrap();
        assert_eq!(settings.mode, WatchMode::Poll);
        assert_eq!(settings.poll_interval, Duration::from_secs(120));
    }

    #[test]
    fn modes_ignore_case_and_empty_values_mean_unset() {
        assert_eq!(
            parse_folder_watching(Some("POLL"), Some("")).unwrap().mode,
            WatchMode::Poll
        );
        let settings = parse_folder_watching(Some(" "), Some(" ")).unwrap();
        assert_eq!(settings.mode, WatchMode::Auto);
        assert_eq!(settings.poll_interval, Duration::from_secs(30));
    }

    #[test]
    fn a_disabled_watcher_does_not_validate_the_interval() {
        assert_eq!(
            parse_folder_watching(Some("off"), Some("soon"))
                .unwrap()
                .mode,
            WatchMode::Off
        );
    }

    #[test]
    fn unknown_modes_and_out_of_range_intervals_stop_startup() {
        assert!(parse_folder_watching(Some("fast"), None).is_err());
        assert!(parse_folder_watching(None, Some("1")).is_err());
        assert!(parse_folder_watching(None, Some("86401")).is_err());
        assert!(parse_folder_watching(None, Some("soon")).is_err());
    }
}
