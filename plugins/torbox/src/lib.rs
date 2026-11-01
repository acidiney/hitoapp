mod audio;
mod catalog;
mod http;
mod journal;
mod policy;
mod preparation;

use hito_debrid_torbox::{MediaAllowlist, TorBoxClient, TorrentFileBinding};
use hito_plugin_sdk::ProviderRecording;
use std::path::PathBuf;

pub struct Config {
    pub state: StateConfig,
    pub service_bearer: String,
    pub client: TorBoxClient,
    pub media_origins: MediaAllowlist,
    pub recordings: Vec<CuratedWavRecording>,
}

pub struct StateConfig {
    pub directory: PathBuf,
    pub key_file: PathBuf,
    pub mode: StateMode,
}

#[derive(Clone, Copy)]
pub enum StateMode {
    CreateNew,
    OpenExisting,
}

pub struct Service {
    router: axum::Router,
    journal: journal::Journal,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum StateError {
    #[error("The private preparation-state configuration is invalid")]
    InvalidConfiguration,
    #[error("The private preparation state is unavailable")]
    Unavailable,
    #[error("The private preparation state is already owned")]
    AlreadyOwned,
    #[error("The private preparation state is invalid")]
    InvalidState,
    #[error("The private preparation-state authority has changed")]
    AuthorityChanged,
}

pub struct CuratedWavRecording {
    pub recording: ProviderRecording,
    pub source_id: String,
    pub label: String,
    pub binding: TorrentFileBinding,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error("The plugin-service credential is invalid")]
    InvalidServiceCredential,
    #[error("The curated canonical-WAV catalog is invalid")]
    InvalidCatalog,
}

impl Service {
    pub async fn open(config: Config) -> Result<Self, InitError> {
        let Config {
            state,
            service_bearer,
            client,
            media_origins,
            recordings,
        } = config;
        let policy = policy::request_policy(service_bearer)?;
        let catalog = catalog::Catalog::new(recordings)?;
        journal::Journal::validate_catalog(&catalog)?;
        let journal = journal::Journal::open(state, &client, &catalog).await?;
        let router = http::router(
            policy,
            catalog,
            client,
            media_origins,
            journal.gate(),
            journal.handle(),
        );
        Ok(Self { router, journal })
    }

    pub fn router(&self) -> axum::Router {
        self.router.clone()
    }

    pub async fn close(self) -> Result<(), StateError> {
        let Self { router, journal } = self;
        drop(router);
        journal.close().await
    }
}
