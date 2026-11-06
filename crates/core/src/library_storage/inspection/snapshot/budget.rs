use super::stopped;
use crate::Result;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const BLOCK: u64 = 4096;

#[derive(Clone)]
pub(super) struct Budget {
    blocks: Arc<Semaphore>,
    total: u32,
}

pub(super) struct Reservation {
    _blocks: OwnedSemaphorePermit,
}

impl Budget {
    pub(super) fn new(bytes: u64) -> Self {
        let total = (bytes / BLOCK).clamp(1, u32::MAX as u64) as u32;
        Self {
            blocks: Arc::new(Semaphore::new(total as usize)),
            total,
        }
    }

    pub(super) async fn reserve(&self, size: u64) -> Result<Reservation> {
        let wanted = size.div_ceil(BLOCK).clamp(1, self.total as u64) as u32;
        let blocks = self
            .blocks
            .clone()
            .acquire_many_owned(wanted)
            .await
            .map_err(|_| stopped())?;
        Ok(Reservation { _blocks: blocks })
    }
}

pub(in crate::library_storage::inspection) struct PrivateCopy {
    _directory: tempfile::TempDir,
    _reservation: Option<Reservation>,
}

impl PrivateCopy {
    pub(super) fn new(directory: tempfile::TempDir, reservation: Option<Reservation>) -> Self {
        Self {
            _directory: directory,
            _reservation: reservation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_file_larger_than_the_budget_waits_for_the_whole_budget_and_frees_it() {
        let budget = Budget::new(10 * BLOCK);
        let small = budget.reserve(3 * BLOCK).await.unwrap();
        let large = tokio::spawn({
            let budget = budget.clone();
            async move { budget.reserve(100 * BLOCK).await }
        });
        tokio::task::yield_now().await;
        assert!(!large.is_finished());
        drop(small);
        let large = large.await.unwrap().unwrap();
        drop(large);
        assert!(budget.reserve(10 * BLOCK).await.is_ok());
    }
}
