use crate::Error;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::Duration,
};
use tokio::sync::watch;

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(super) struct Admission {
    pub(super) state: Mutex<State>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) active_pending: HashMap<String, usize>,
    pub(super) tickets: HashMap<u64, Weak<WarmTicket>>,
    pub(super) global_active_pending: usize,
}

pub(crate) struct WarmTicket {
    id: u64,
    account: String,
    admission: Weak<Admission>,
    cancel: watch::Sender<bool>,
    done: watch::Sender<bool>,
    claimed: AtomicBool,
    preparing: AtomicBool,
}

pub(super) struct Active<'a> {
    admission: &'a Admission,
    account: String,
}

impl Admission {
    pub(super) fn warm_resources(
        self: &Arc<Self>,
        account: &str,
        accounts: &crate::admission::Accounts,
        slots: &Arc<tokio::sync::Semaphore>,
    ) -> Result<(Arc<WarmTicket>, tokio::sync::OwnedSemaphorePermit, tokio::sync::OwnedSemaphorePermit), Error> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("Audio admission is unavailable".into()))?;
        state.tickets.retain(|_, ticket| ticket.strong_count() > 0);
        let account_count = state
            .tickets
            .values()
            .filter_map(Weak::upgrade)
            .filter(|ticket| ticket.account == account)
            .count();
        if state.global_active_pending > 0
            || state.active_pending.contains_key(account)
            || account_count >= 1
            || state.tickets.len() >= 2
        {
            return Err(Error::Conflict("Audio streams are busy. Try again.".into()));
        }
        let account_permit = accounts
            .try_enter(account)?
            .ok_or_else(|| Error::Conflict("Audio streams are busy. Try again.".into()))?;
        let global_permit = slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Audio streams are busy. Try again.".into()))?;
        let (cancel, _) = watch::channel(false);
        let (done, _) = watch::channel(false);
        let ticket = Arc::new(WarmTicket {
            id: NEXT_TICKET.fetch_add(1, Ordering::Relaxed),
            account: account.into(),
            admission: Arc::downgrade(self),
            cancel,
            done,
            claimed: AtomicBool::new(false),
            preparing: AtomicBool::new(true),
        });
        state.tickets.insert(ticket.id, Arc::downgrade(&ticket));
        Ok((ticket, account_permit, global_permit))
    }

    pub(super) fn active(&self, account: &str, slots: &tokio::sync::Semaphore) -> Result<(Active<'_>, Vec<watch::Receiver<bool>>), Error> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("Audio admission is unavailable".into()))?;
        state.tickets.retain(|_, ticket| ticket.strong_count() > 0);
        *state.active_pending.entry(account.into()).or_default() += 1;
        state.global_active_pending += 1;
        let global_full = slots.available_permits() == 0;
        let mut waiting = Vec::new();
        for ticket in state.tickets.values().filter_map(Weak::upgrade) {
            if ticket.claimed.load(Ordering::Acquire) {
                continue;
            }
            if ticket.account == account || global_full {
                ticket.cancel.send_replace(true);
                waiting.push(ticket.done.subscribe());
            }
        }
        Ok((Active { admission: self, account: account.into() }, waiting))
    }

    pub(super) fn preempt_global(&self) -> Result<Vec<watch::Receiver<bool>>, Error> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("Audio admission is unavailable".into()))?;
        state.tickets.retain(|_, ticket| ticket.strong_count() > 0);
        let mut waiting = Vec::new();
        for ticket in state.tickets.values().filter_map(Weak::upgrade) {
            if ticket.claimed.load(Ordering::Acquire) {
                continue;
            }
            waiting.push(ticket.done.subscribe());
            ticket.cancel.send_replace(true);
        }
        Ok(waiting)
    }

    pub(super) fn claim(&self, ticket: &WarmTicket) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        if state.active_pending.contains_key(&ticket.account)
            || *ticket.cancel.borrow()
            || ticket.claimed.swap(true, Ordering::AcqRel)
        {
            return false;
        }
        true
    }
}

impl WarmTicket {
    pub(crate) fn cancelled(&self) -> watch::Receiver<bool> {
        self.cancel.subscribe()
    }

    pub(crate) fn claim(&self) -> bool {
        self.admission
            .upgrade()
            .is_some_and(|admission| admission.claim(self))
    }

    pub(crate) fn is_claimed(&self) -> bool {
        self.claimed.load(Ordering::Acquire)
    }

    pub(crate) fn is_preparing(&self) -> bool {
        self.preparing.load(Ordering::Acquire)
    }

    pub(crate) fn finish_preparation(&self) {
        self.preparing.store(false, Ordering::Release);
    }

}

impl Drop for WarmTicket {
    fn drop(&mut self) {
        self.done.send_replace(true);
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.admission.state.lock() {
            state.global_active_pending = state.global_active_pending.saturating_sub(1);
            if let Some(count) = state.active_pending.get_mut(&self.account) {
                *count -= 1;
                if *count == 0 {
                    state.active_pending.remove(&self.account);
                }
            }
        }
    }
}

pub(super) async fn await_preemptions(
    waiting: &mut [watch::Receiver<bool>],
    budget: Duration,
) -> bool {
    tokio::time::timeout(budget, async {
        for receiver in waiting {
            let mut receiver = receiver.clone();
            if !*receiver.borrow() && receiver.changed().await.is_err() && !*receiver.borrow() {
                return false;
            }
        }
        true
    })
    .await
    .unwrap_or(false)
}

