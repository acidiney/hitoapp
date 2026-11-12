mod stream;
#[cfg(test)]
mod tests;

use crate::{Actor, Error, Result, Store};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};
pub use stream::ChangeStream;
use tokio::sync::broadcast;
use utoipa::ToSchema;

const NOTICE_CAPACITY: usize = 32;
const STREAMS_PER_ACCOUNT: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeNotice {
    Resync {
        reason: ResyncReason,
    },
    Sources {
        recording_id: String,
    },
    Recording {
        recording_id: String,
    },
    Recordings,
    Playlist {
        playlist_id: String,
    },
    Playlists,
    Artwork {
        entity: ArtworkChangeEntity,
        id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    Connected,
    Lagged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkChangeEntity {
    Recording,
    Release,
    Playlist,
}

impl ChangeNotice {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Resync { .. } => "resync",
            Self::Sources { .. } => "sources",
            Self::Recording { .. } => "recording",
            Self::Recordings => "recordings",
            Self::Playlist { .. } => "playlist",
            Self::Playlists => "playlists",
            Self::Artwork { .. } => "artwork",
        }
    }
}

#[derive(Clone, Debug)]
enum Signal {
    Notice(ChangeNotice),
    SessionsChanged,
}

struct Channel {
    sender: broadcast::Sender<Signal>,
}

#[derive(Clone, Default)]
pub(crate) struct ChangeHub {
    accounts: Arc<Mutex<HashMap<String, Channel>>>,
}

impl ChangeHub {
    pub(crate) fn publish(&self, account: &str, notices: impl IntoIterator<Item = ChangeNotice>) {
        let mut accounts = self.accounts();
        let Some(channel) = live(&mut accounts, account) else {
            return;
        };
        let mut published = 0;
        for notice in notices {
            if channel.sender.send(Signal::Notice(notice)).is_ok() {
                published += 1;
            }
        }
        if published > 0 {
            tracing::debug!(
                event = "change_notices_published",
                notices = published,
                subscribers = channel.sender.receiver_count(),
                "Change notices published"
            );
        }
    }

    pub(crate) fn sessions_changed(&self, account: &str) {
        let mut accounts = self.accounts();
        if let Some(channel) = live(&mut accounts, account) {
            let _ = channel.sender.send(Signal::SessionsChanged);
        }
    }

    fn register(&self, account: &str) -> Result<Registration> {
        let mut accounts = self.accounts();
        let channel = accounts
            .entry(account.to_owned())
            .or_insert_with(|| Channel {
                sender: broadcast::channel(NOTICE_CAPACITY).0,
            });
        if channel.sender.receiver_count() >= STREAMS_PER_ACCOUNT {
            return Err(Error::RateLimited(5));
        }
        Ok(Registration {
            hub: self.clone(),
            account: account.to_owned(),
            receiver: Some(channel.sender.subscribe()),
        })
    }

    fn release(&self, account: &str) {
        let mut accounts = self.accounts();
        live(&mut accounts, account);
    }

    #[cfg(test)]
    fn registrations(&self, account: &str) -> Option<usize> {
        self.accounts()
            .get(account)
            .map(|channel| channel.sender.receiver_count())
    }

    fn accounts(&self) -> MutexGuard<'_, HashMap<String, Channel>> {
        self.accounts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

struct Registration {
    hub: ChangeHub,
    account: String,
    receiver: Option<broadcast::Receiver<Signal>>,
}

impl Registration {
    fn close(&mut self) {
        if let Some(receiver) = self.receiver.take() {
            drop(receiver);
            self.hub.release(&self.account);
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.close();
    }
}

fn live<'a>(accounts: &'a mut HashMap<String, Channel>, account: &str) -> Option<&'a Channel> {
    if accounts
        .get(account)
        .is_some_and(|channel| channel.sender.receiver_count() == 0)
    {
        accounts.remove(account);
    }
    accounts.get(account)
}

#[derive(Clone)]
pub struct Changes {
    pub(crate) store: Store,
    pub(crate) hub: ChangeHub,
}

impl Changes {
    pub async fn subscribe(&self, actor: &Actor) -> Result<ChangeStream> {
        actor.require_unlocked()?;
        actor.require_foreground_session()?;
        let registration = self.hub.register(actor.id())?;
        actor.check_session(&self.store).await?;
        Ok(ChangeStream::new(self.clone(), actor.clone(), registration))
    }
}
