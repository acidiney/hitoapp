use super::{ReplicaSession, Replicas, files, transport::RemoteResult};
use crate::{Error, Result};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub(super) struct LeaseSignal {
    pub connection: Uuid,
    pub stopped: tokio::sync::watch::Sender<bool>,
}

impl Replicas {
    pub(super) fn register_lease(
        &self,
        connection: Uuid,
        activation: Uuid,
    ) -> Result<Arc<LeaseSignal>> {
        let mut leases = self
            .profiles
            .signals
            .lock()
            .map_err(|_| files::unavailable())?;
        leases.retain(|_, signal| {
            signal
                .upgrade()
                .is_some_and(|signal| !*signal.stopped.borrow())
        });
        if leases.len() >= 256 {
            return Err(Error::Conflict("Too many active replica sessions".into()));
        }
        let signal = Arc::new(LeaseSignal {
            connection,
            stopped: tokio::sync::watch::channel(false).0,
        });
        leases.insert(activation, Arc::downgrade(&signal));
        Ok(signal)
    }

    pub(super) fn retire_activation(&self, activation: Uuid) -> Result<()> {
        let signal = self
            .profiles
            .signals
            .lock()
            .map_err(|_| files::unavailable())?
            .remove(&activation)
            .and_then(|signal| signal.upgrade());
        if let Some(signal) = signal {
            signal.stopped.send_replace(true);
        }
        Ok(())
    }

    pub(super) fn retire_connection(&self, connection: Uuid) -> Result<()> {
        let ids: Vec<_> = self
            .profiles
            .signals
            .lock()
            .map_err(|_| files::unavailable())?
            .iter()
            .filter_map(|(id, signal)| {
                signal
                    .upgrade()
                    .filter(|signal| signal.connection == connection)
                    .map(|_| *id)
            })
            .collect();
        for id in ids {
            self.retire_activation(id)?;
        }
        Ok(())
    }

    pub(super) async fn join_connection(&self, connection: Uuid) -> Result<()> {
        drop(
            self.profiles
                .synchronizing
                .enter_within(&connection.to_string(), Duration::from_secs(2))
                .await?,
        );
        Ok(())
    }

    pub(super) async fn remote_session_call<T>(
        &self,
        session: &ReplicaSession,
        call: impl std::future::Future<Output = RemoteResult<T>>,
    ) -> Result<T> {
        let mut retired = session.signal.stopped.subscribe();
        if *retired.borrow() {
            return Err(Error::Conflict(
                "This replica session has been retired".into(),
            ));
        }
        tokio::select! {
            biased;
            _=retired.changed()=>Err(Error::Conflict("This replica session has been retired".into())),
            result=self.remote_call(call)=>result,
        }
    }
}
