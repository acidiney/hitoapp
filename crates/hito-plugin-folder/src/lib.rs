mod audio;
mod bindings;
mod catalog;
mod extract;
mod http;
mod inspection;
mod pcm;
mod registry;
mod releases;
mod scan;
mod search;
mod stream;

use axum::Router;
use std::{io, path::Path, sync::Arc};

pub struct Folder(Arc<catalog::Catalog>);
impl Folder {
    pub fn scan(path: &Path) -> io::Result<Self> {
        scan::scan(path).map(|catalog| Self(Arc::new(catalog)))
    }
    pub async fn initialize(path: &Path, state: &Path) -> io::Result<()> {
        let registry = registry::Registry::open(state, true).await?;
        Self::commit_scan(path, registry).await.map(|_| ())
    }
    pub async fn open(path: &Path, state: &Path) -> io::Result<Self> {
        let registry = registry::Registry::open(state, false).await?;
        Self::commit_scan(path, registry).await
    }
    async fn commit_scan(path: &Path, registry: registry::Registry) -> io::Result<Self> {
        let path = path.to_owned();
        let claim = registry.claim();
        let scanned = tokio::task::spawn_blocking(move || scan::scan_owned(&path, claim))
            .await
            .map_err(|_| io::Error::other("Folder scan worker failed"))
            .and_then(|result| result);
        let mut catalog = match scanned {
            Ok(catalog) => catalog,
            Err(error) => {
                registry.close().await?;
                return Err(error);
            }
        };
        registry.commit(&mut catalog).await?;
        Ok(Self(Arc::new(catalog)))
    }
    pub fn router(self) -> Router {
        http::router(self.0)
    }
}
