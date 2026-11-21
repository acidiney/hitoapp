mod listing;
mod review;
mod submission;
pub(crate) mod types;

use std::sync::Arc;

use crate::storage::Storage;

#[derive(Clone)]
pub(crate) struct Corrections {
    storage: Arc<Storage>,
    limits: crate::Limits,
}

impl Corrections {
    pub(crate) fn new(storage: Arc<Storage>, limits: crate::Limits) -> Self {
        Self { storage, limits }
    }
}
