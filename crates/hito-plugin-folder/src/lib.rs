mod audio;
mod catalog;
mod extract;
mod http;
mod inspection;
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
    pub fn router(self) -> Router {
        http::router(self.0)
    }
}
