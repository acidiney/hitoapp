use crate::{LocalHost, RemoteHost, Result};
use url::Url;

pub enum AppHost {
    Local(LocalHost),
    Remote(RemoteHost),
}

impl AppHost {
    pub fn entry_url(&self) -> Url {
        match self {
            Self::Local(host) => host.entry_url(),
            Self::Remote(host) => host.entry_url(),
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        match self {
            Self::Local(host) => host.shutdown().await,
            Self::Remote(host) => host.shutdown().await,
        }
    }
}
