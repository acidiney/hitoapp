use hito_community::{Config, Limits};
use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub(super) struct Settings {
    pub listen: SocketAddr,
    pub community: Config,
}

impl Settings {
    pub(super) fn read() -> Result<Self, &'static str> {
        let mut listen = SocketAddr::from(([127, 0, 0, 1], 8095));
        let mut listen_seen = false;
        let mut state_directory: Option<PathBuf> = None;
        let mut credential: Option<PathBuf> = None;
        let mut correction_recent: Option<u32> = None;
        let mut correction_pending: Option<u32> = None;
        let mut correction_total: Option<u32> = None;
        let mut outcome_recent: Option<u32> = None;
        let mut arguments = std::env::args_os().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--listen") if !listen_seen => {
                    listen = arguments
                        .next()
                        .and_then(|value| value.into_string().ok())
                        .ok_or("missing_listen_address")?
                        .parse()
                        .map_err(|_| "invalid_listen_address")?;
                    listen_seen = true;
                }
                Some("--state-directory") if state_directory.is_none() => {
                    state_directory = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_state_directory")?,
                    );
                }
                Some("--admin-token-file") if credential.is_none() => {
                    credential = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_admin_token_file")?,
                    );
                }
                Some("--correction-recent-per-minute") if correction_recent.is_none() => {
                    correction_recent = Some(limit(arguments.next())?);
                }
                Some("--correction-pending-per-reporter") if correction_pending.is_none() => {
                    correction_pending = Some(limit(arguments.next())?);
                }
                Some("--correction-pending-total") if correction_total.is_none() => {
                    correction_total = Some(limit(arguments.next())?);
                }
                Some("--outcome-recent-per-minute") if outcome_recent.is_none() => {
                    outcome_recent = Some(limit(arguments.next())?);
                }
                _ => return Err("invalid_arguments"),
            }
        }
        let state_directory = state_directory.ok_or("missing_state_directory")?;
        if !state_directory.is_absolute() || state_directory.file_name().is_none() {
            return Err("invalid_state_directory_path");
        }
        let credential = credential.ok_or("missing_admin_token_file")?;
        if !credential.is_absolute() {
            return Err("invalid_admin_token_file_path");
        }
        let admin_token = read_admin_token(&credential)?;
        let defaults = Limits::default();
        Ok(Self {
            listen,
            community: Config {
                state_directory,
                admin_token,
                limits: Limits {
                    correction_recent_per_minute: correction_recent
                        .unwrap_or(defaults.correction_recent_per_minute),
                    correction_pending_per_reporter: correction_pending
                        .unwrap_or(defaults.correction_pending_per_reporter),
                    correction_pending_total: correction_total
                        .unwrap_or(defaults.correction_pending_total),
                    outcome_recent_per_minute: outcome_recent
                        .unwrap_or(defaults.outcome_recent_per_minute),
                },
            },
        })
    }
}

fn limit(value: Option<OsString>) -> Result<u32, &'static str> {
    value
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|limit| *limit > 0)
        .ok_or("invalid_limit")
}

#[cfg(unix)]
fn read_admin_token(path: &Path) -> Result<String, &'static str> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    use zeroize::Zeroizing;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "admin_token_file_unavailable")?;
    let metadata = file
        .metadata()
        .map_err(|_| "admin_token_file_unavailable")?;
    let mode = metadata.permissions().mode() & 0o7777;
    let owner = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner
        || !matches!(mode, 0o400 | 0o600)
        || !(64..=66).contains(&metadata.len())
    {
        return Err("admin_token_file_not_private");
    }
    let mut token = Zeroizing::new(String::new());
    file.take(67)
        .read_to_string(&mut token)
        .map_err(|_| "admin_token_file_invalid")?;
    if token.ends_with('\n') {
        token.pop();
        if token.ends_with('\r') {
            token.pop();
        }
    }
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("admin_token_file_invalid");
    }
    Ok(std::mem::take(&mut *token))
}

#[cfg(not(unix))]
fn read_admin_token(_path: &Path) -> Result<String, &'static str> {
    Err("admin_token_platform_unsupported")
}
