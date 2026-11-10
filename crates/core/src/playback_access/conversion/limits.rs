use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) const ACCOUNT_CONVERSIONS: u32 = 3;
const BUSY: &str = "Too many audio conversions are running. Try again in a moment.";

#[derive(Clone)]
pub(crate) struct ConversionLimits {
    host: Arc<Semaphore>,
    accounts: Arc<Mutex<HashMap<String, u32>>>,
    per_account: u32,
}

pub(crate) struct ConversionSlot {
    _host: OwnedSemaphorePermit,
    accounts: Arc<Mutex<HashMap<String, u32>>>,
    account: String,
}

impl ConversionLimits {
    pub(crate) fn new(host: usize) -> Self {
        let host = host.clamp(1, 8);
        Self {
            host: Arc::new(Semaphore::new(host)),
            accounts: Arc::default(),
            per_account: ACCOUNT_CONVERSIONS.min(host as u32),
        }
    }

    pub(crate) fn acquire(&self, account: &str) -> Result<ConversionSlot> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        let running = accounts.get(account).copied().unwrap_or(0);
        if running >= self.per_account {
            return Err(Error::ProviderBusy(BUSY));
        }
        let host = self
            .host
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::ProviderBusy(BUSY))?;
        accounts.insert(account.to_owned(), running + 1);
        Ok(ConversionSlot {
            _host: host,
            accounts: self.accounts.clone(),
            account: account.to_owned(),
        })
    }
}

impl Drop for ConversionSlot {
    fn drop(&mut self) {
        let mut accounts = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(running) = accounts.get_mut(&self.account) {
            *running -= 1;
            if *running == 0 {
                accounts.remove(&self.account);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_is_capped_before_the_host_is() {
        let limits = ConversionLimits::new(8);
        let held: Vec<_> = (0..3).map(|_| limits.acquire("a").unwrap()).collect();
        assert!(matches!(limits.acquire("a"), Err(Error::ProviderBusy(_))));
        assert!(limits.acquire("b").is_ok());
        drop(held);
        assert!(limits.acquire("a").is_ok());
    }

    #[test]
    fn the_host_cap_applies_across_accounts() {
        let limits = ConversionLimits::new(2);
        let first = limits.acquire("a").unwrap();
        let _second = limits.acquire("b").unwrap();
        assert!(matches!(limits.acquire("c"), Err(Error::ProviderBusy(_))));
        drop(first);
        assert!(limits.acquire("c").is_ok());
    }

    #[test]
    fn a_rejected_request_does_not_leak_a_slot() {
        let limits = ConversionLimits::new(1);
        let held = limits.acquire("a").unwrap();
        assert!(limits.acquire("a").is_err());
        assert!(limits.acquire("b").is_err());
        drop(held);
        assert!(limits.acquire("b").is_ok());
    }
}
