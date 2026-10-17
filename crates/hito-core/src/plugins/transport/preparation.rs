use super::{Admission, ControlClient};
use crate::{Error, Result};

impl ControlClient {
    pub(in crate::plugins) fn admit_preparation(&self, account: &str) -> Result<Admission> {
        let account = self.preparation_accounts.enter(account)?;
        let global = self
            .preparation_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                Error::Conflict("Source preparation requests are busy. Try again.".into())
            })?;
        Ok(Admission {
            _account: account,
            _global: global,
        })
    }
}
