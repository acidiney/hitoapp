mod listing;
mod review;
mod submission;
pub(crate) mod types;

use std::sync::Arc;

use crate::storage::Storage;

#[derive(Clone)]
pub(crate) struct Corrections {
    storage: Arc<Storage>,
}

impl Corrections {
    pub(crate) fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}
