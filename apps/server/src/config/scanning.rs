use super::RuntimeResult;
use hito_core::MediaTools;
use std::path::PathBuf;

const MAX_PARALLELISM: usize = 64;
const MAX_BUDGET_MIB: u64 = 1_048_576;

pub(super) struct ScanSettings<'a> {
    pub parallelism: Option<&'a str>,
    pub temp_budget_mib: Option<&'a str>,
    pub temp_dir: Option<&'a str>,
}

pub(super) fn apply_scan_settings(
    tools: &mut MediaTools,
    settings: ScanSettings<'_>,
) -> RuntimeResult<()> {
    if let Some(value) = text(settings.parallelism) {
        tools.parallelism = value
            .parse::<usize>()
            .ok()
            .filter(|count| (1..=MAX_PARALLELISM).contains(count))
            .ok_or_else(|| invalid("HITO_SCAN_PARALLELISM must be a whole number from 1 to 64"))?;
    }
    if let Some(value) = text(settings.temp_budget_mib) {
        let mib = value
            .parse::<u64>()
            .ok()
            .filter(|mib| (1..=MAX_BUDGET_MIB).contains(mib))
            .ok_or_else(|| {
                invalid("HITO_SCAN_TEMP_BUDGET_MIB must be a whole number from 1 to 1048576")
            })?;
        tools.temp_budget = mib * 1024 * 1024;
    }
    if let Some(value) = text(settings.temp_dir) {
        tools.temp_dir = Some(PathBuf::from(value));
    }
    Ok(())
}

fn text(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn invalid(message: &'static str) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(
        parallelism: Option<&str>,
        temp_budget_mib: Option<&str>,
        temp_dir: Option<&str>,
    ) -> RuntimeResult<MediaTools> {
        let mut tools = MediaTools::default();
        apply_scan_settings(
            &mut tools,
            ScanSettings {
                parallelism,
                temp_budget_mib,
                temp_dir,
            },
        )?;
        Ok(tools)
    }

    #[test]
    fn unset_and_empty_values_keep_the_defaults() {
        let defaults = MediaTools::default();
        let tools = apply(None, Some(" "), Some("")).unwrap();
        assert_eq!(tools.parallelism, defaults.parallelism);
        assert_eq!(tools.temp_budget, 2 * 1024 * 1024 * 1024);
        assert_eq!(tools.temp_dir, None);
    }

    #[test]
    fn operators_set_the_parallelism_budget_and_directory() {
        let tools = apply(Some("3"), Some("512"), Some("/scratch/hito")).unwrap();
        assert_eq!(tools.parallelism, 3);
        assert_eq!(tools.temp_budget, 512 * 1024 * 1024);
        assert_eq!(tools.temp_dir, Some(PathBuf::from("/scratch/hito")));
    }

    #[test]
    fn out_of_range_or_malformed_values_stop_startup() {
        for value in ["0", "65", "many", "-1"] {
            assert!(apply(Some(value), None, None).is_err(), "{value}");
        }
        for value in ["0", "1048577", "big", "1.5"] {
            assert!(apply(None, Some(value), None).is_err(), "{value}");
        }
    }
}
