use hito_plugin_lrclib::Config;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use url::Url;

pub(super) struct Settings {
    pub listen: SocketAddr,
    pub config: Config,
}

impl Settings {
    pub(super) fn read() -> Result<Self, &'static str> {
        let mut listen = SocketAddr::from(([127, 0, 0, 1], 8101));
        let (mut listen_seen, mut token_file, mut endpoint) = (false, None::<PathBuf>, None::<Url>);
        let mut arguments = std::env::args_os().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--listen") if !listen_seen => {
                    listen = arguments
                        .next()
                        .and_then(|value| value.into_string().ok())
                        .ok_or("missing_listen")?
                        .parse()
                        .map_err(|_| "invalid_listen")?;
                    listen_seen = true;
                }
                Some("--service-token-file") if token_file.is_none() => {
                    token_file = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_service_token_file")?,
                    )
                }
                Some("--endpoint") if endpoint.is_none() => {
                    endpoint = Some(
                        Url::parse(
                            &arguments
                                .next()
                                .and_then(|value| value.into_string().ok())
                                .ok_or("missing_endpoint")?,
                        )
                        .map_err(|_| "invalid_endpoint")?,
                    )
                }
                _ => return Err("invalid_arguments"),
            }
        }
        let token_file = token_file.ok_or("missing_service_token_file")?;
        if !token_file.is_absolute() {
            return Err("service_token_path_not_absolute");
        }
        let config = read_credential(&token_file)?;
        let config = match endpoint {
            Some(endpoint) => config
                .with_endpoint(endpoint)
                .map_err(|_| "invalid_endpoint")?,
            None => config,
        };
        let config = match std::env::var("HITO_METADATA_CONTACT") {
            Ok(contact) if !contact.trim().is_empty() => config
                .with_contact(contact.trim())
                .map_err(|_| "invalid_metadata_contact")?,
            Ok(_) | Err(std::env::VarError::NotPresent) => config,
            Err(std::env::VarError::NotUnicode(_)) => return Err("invalid_metadata_contact"),
        };
        Ok(Self { listen, config })
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
        .map_err(|_| "service_token_unavailable")?;
    let metadata = file.metadata().map_err(|_| "service_token_unavailable")?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || !matches!(metadata.permissions().mode() & 0o7777, 0o400 | 0o600)
        || !(1..=4098).contains(&metadata.len())
    {
        return Err("service_token_not_private");
    }
    let mut token = Zeroizing::new(String::new());
    file.take(4099)
        .read_to_string(&mut token)
        .map_err(|_| "service_token_invalid")?;
    if token.ends_with('\n') {
        token.pop();
        if token.ends_with('\r') {
            token.pop();
        }
    }
    Config::new(&token).map_err(|_| "service_token_invalid")
}

#[cfg(not(unix))]
fn read_credential(_path: &Path) -> Result<Config, &'static str> {
    Err("private_service_tokens_unsupported")
}
