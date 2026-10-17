mod admission;
mod config;
mod contract;
mod error;
mod http;
mod outcomes;
mod rankings;
mod storage;
mod telemetry;
mod wire;

use std::sync::Arc;

pub use config::Config;
pub use error::Error;
pub use http::router;

#[derive(Clone)]
pub struct Service {
    pub(crate) admission: admission::Admission,
    pub(crate) outcomes: outcomes::Outcomes,
    pub(crate) rankings: rankings::Rankings,
    storage: Arc<storage::Storage>,
}

impl Service {
    pub async fn open(config: Config) -> Result<Self, Error> {
        let (path, admin_hash) = config.validate()?;
        let storage = Arc::new(storage::Storage::open(path).await?);
        Ok(Self {
            admission: admission::Admission::new(storage.clone(), admin_hash),
            outcomes: outcomes::Outcomes::new(storage.clone()),
            rankings: rankings::Rankings::new(storage.clone()),
            storage,
        })
    }

    pub async fn close(&self) {
        self.storage.close().await;
    }
}
