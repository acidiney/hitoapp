mod audio;
mod bindings;
mod catalog;
mod extract;
mod http;
mod inspection;
mod pcm;
mod registry;
mod releases;
mod representation;
mod scan;
mod search;
mod startup;
mod stream;

use axum::Router;
use std::{io, path::Path, sync::Arc};

pub struct Folder(Arc<catalog::Catalog>);
impl Folder {
    pub fn scan(path: &Path) -> io::Result<Self> {
        scan::scan(path).map(|catalog| Self(Arc::new(catalog)))
    }
    pub async fn initialize(path: &Path, state: &Path) -> io::Result<()> {
        startup::load(path, state, true).await.map(|_| ())
    }
    pub async fn open(path: &Path, state: &Path) -> io::Result<Self> {
        startup::load(path, state, false)
            .await
            .map(|catalog| Self(Arc::new(catalog)))
    }
    pub fn router(self) -> Router {
        http::router(self.0)
    }
}
