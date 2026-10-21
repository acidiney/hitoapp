use super::http;
use hito_plugin_torbox::{Config, Service, StateConfig, StateMode};
use std::{cell::Cell, fs::OpenOptions, io::Write, ops::Deref};
use tracing::instrument::WithSubscriber;

pub struct Inputs {
    pub(super) directory: tempfile::TempDir,
    next_mode: Cell<StateMode>,
}

impl Default for Inputs {
    fn default() -> Self {
        Self::new()
    }
}

impl Inputs {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut key = options
            .open(directory.path().join("candidate.key"))
            .unwrap();
        key.write_all(&[73_u8; 32]).unwrap();
        key.sync_all().unwrap();
        Self {
            directory,
            next_mode: Cell::new(StateMode::CreateNew),
        }
    }

    pub fn next(&self) -> StateConfig {
        StateConfig {
            directory: self.directory.path().join("journal"),
            key_file: self.directory.path().join("candidate.key"),
            mode: self.next_mode.replace(StateMode::OpenExisting),
        }
    }
}

pub struct Server {
    inner: http::Server,
    owner: Service,
    inputs: Option<Inputs>,
}

impl Server {
    pub async fn start(
        config: Config,
        dispatch: tracing::Dispatch,
        inputs: Option<Inputs>,
    ) -> Self {
        let owner = Service::open(config)
            .with_subscriber(dispatch.clone())
            .await
            .unwrap();
        let inner = http::Server::start(owner.router(), dispatch).await;
        Self {
            inner,
            owner,
            inputs,
        }
    }

    pub async fn close(self) -> Result<(), &'static str> {
        let Self {
            inner,
            owner,
            inputs,
        } = self;
        let server = inner.close().await;
        let state = owner.close().await;
        drop(inputs);
        server?;
        state.map_err(|_| "owned plugin state failed to join shutdown")
    }
}

impl Deref for Server {
    type Target = http::Server;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
