use crate::{Error, Result};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(crate) struct ActiveJobs(Arc<Mutex<HashSet<String>>>);

pub(super) struct ActiveJob {
    jobs: ActiveJobs,
    token: String,
}

impl ActiveJobs {
    pub(super) fn contains(&self, token: &str) -> Result<bool> {
        Ok(self.0.lock().map_err(|_| stopped())?.contains(token))
    }

    pub(super) fn enter(&self, token: &str) -> Result<ActiveJob> {
        self.0.lock().map_err(|_| stopped())?.insert(token.into());
        Ok(ActiveJob {
            jobs: self.clone(),
            token: token.into(),
        })
    }
}

impl Drop for ActiveJob {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.0.lock() {
            jobs.remove(&self.token);
        }
    }
}

fn stopped() -> Error {
    Error::Internal("Download workers stopped".into())
}
