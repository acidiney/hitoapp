use hito_plugin_jamendo::Config;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub(super) struct Settings {
    pub listen: SocketAddr,
    pub provider: Config,
}

impl Settings {
    pub(super) fn read() -> Result<Self, &'static str> {
        let mut listen = SocketAddr::from(([127, 0, 0, 1], 8093));
        let mut listen_seen = false;
        let mut credential: Option<PathBuf> = None;
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
                Some("--client-id-file") if credential.is_none() => {
                    credential = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_client_id_file")?,
                    );
                }
                _ => return Err("invalid_arguments"),
            }
        }
        let credential = credential.ok_or("missing_client_id_file")?;
        if !credential.is_absolute() {
            return Err("invalid_client_id_file_path");
        }
        let provider = read_credential(&credential)?;
        Ok(Self { listen, provider })
    }
}

#[cfg(unix)]
fn read_credential(path: &Path) -> Result<Config, &'static str> {
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
        .map_err(|_| "credential_file_unavailable")?;
    let metadata = file.metadata().map_err(|_| "credential_file_unavailable")?;
    let mode = metadata.permissions().mode() & 0o7777;
    let owner = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner
        || !matches!(mode, 0o400 | 0o600)
        || !(1..=130).contains(&metadata.len())
    {
        return Err("credential_file_not_private");
    }
    let mut client_id = Zeroizing::new(String::new());
    file.take(131)
        .read_to_string(&mut client_id)
        .map_err(|_| "credential_file_invalid")?;
    if client_id.ends_with('\n') {
        client_id.pop();
        if client_id.ends_with('\r') {
            client_id.pop();
        }
    }
    Config::new(std::mem::take(&mut *client_id)).map_err(|_| "credential_file_invalid")
}

#[cfg(not(unix))]
fn read_credential(_path: &Path) -> Result<Config, &'static str> {
    Err("credential_platform_unsupported")
}
