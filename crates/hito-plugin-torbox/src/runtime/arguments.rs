use hito_plugin_torbox::StateMode;
use std::{net::SocketAddr, path::PathBuf};

pub(super) struct Arguments {
    pub listen: SocketAddr,
    pub provider: PathBuf,
    pub service: PathBuf,
    pub key: PathBuf,
    pub state: PathBuf,
    pub mode: StateMode,
    pub catalog: PathBuf,
}

impl Arguments {
    pub(super) fn read() -> Result<Self, &'static str> {
        let mut listen = SocketAddr::from(([127, 0, 0, 1], 8100));
        let mut listen_seen = false;
        let mut provider = None;
        let mut service = None;
        let mut key = None;
        let mut state = None;
        let mut mode = None;
        let mut catalog = None;
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
                Some("--provider-token-file") if provider.is_none() => {
                    provider = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_provider_token_file")?,
                    );
                }
                Some("--service-token-file") if service.is_none() => {
                    service = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_service_token_file")?,
                    );
                }
                Some("--state-key-file") if key.is_none() => {
                    key = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_state_key_file")?,
                    );
                }
                Some("--state-directory") if state.is_none() => {
                    state = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_state_directory")?,
                    );
                }
                Some("--state-mode") if mode.is_none() => {
                    let value = arguments.next().ok_or("missing_state_mode")?;
                    mode = Some(match value.to_str() {
                        Some("create-new") => StateMode::CreateNew,
                        Some("open-existing") => StateMode::OpenExisting,
                        _ => return Err("invalid_state_mode"),
                    });
                }
                Some("--catalog-file") if catalog.is_none() => {
                    catalog = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_catalog_file")?,
                    );
                }
                _ => return Err("invalid_arguments"),
            }
        }
        Ok(Self {
            listen,
            provider: provider.ok_or("missing_provider_token_file")?,
            service: service.ok_or("missing_service_token_file")?,
            key: key.ok_or("missing_state_key_file")?,
            state: state.ok_or("missing_state_directory")?,
            mode: mode.ok_or("missing_state_mode")?,
            catalog: catalog.ok_or("missing_catalog_file")?,
        })
    }
}
