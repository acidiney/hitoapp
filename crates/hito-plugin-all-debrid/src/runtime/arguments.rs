use std::{net::SocketAddr, path::PathBuf};

pub(super) struct Arguments {
    pub listen: SocketAddr,
    pub provider: PathBuf,
    pub service: PathBuf,
    pub receipt: PathBuf,
    pub catalog: PathBuf,
}

impl Arguments {
    pub(super) fn read() -> Result<Self, &'static str> {
        let mut listen = SocketAddr::from(([127, 0, 0, 1], 8097));
        let mut listen_seen = false;
        let mut provider = None;
        let mut service = None;
        let mut receipt = None;
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
                Some("--receipt-key-file") if receipt.is_none() => {
                    receipt = Some(
                        arguments
                            .next()
                            .map(PathBuf::from)
                            .ok_or("missing_receipt_key_file")?,
                    );
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
            receipt: receipt.ok_or("missing_receipt_key_file")?,
            catalog: catalog.ok_or("missing_catalog_file")?,
        })
    }
}
