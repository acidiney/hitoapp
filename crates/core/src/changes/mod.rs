mod stream;

use crate::{Actor, Error, Result, Store};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
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

#[derive(Clone, Default)]
pub(crate) struct ChangeHub {
    accounts: Arc<Mutex<HashMap<String, broadcast::Sender<Signal>>>>,
}

impl ChangeHub {
    pub(crate) fn publish(&self, account: &str, notices: impl IntoIterator<Item = ChangeNotice>) {
        let (subscribers, published) = self.send(account, notices.into_iter().map(Signal::Notice));
        if published > 0 {
            tracing::debug!(
                event = "change_notices_published",
                notices = published,
                subscribers,
                "Change notices published"
            );
        }
    }

    pub(crate) fn sessions_changed(&self, account: &str) {
        self.send(account, [Signal::SessionsChanged]);
    }

    fn send(&self, account: &str, signals: impl IntoIterator<Item = Signal>) -> (usize, usize) {
        let mut accounts = self
            .accounts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(sender) = accounts.get(account) else {
            return (0, 0);
        };
        let subscribers = sender.receiver_count();
        if subscribers == 0 {
            accounts.remove(account);
            return (0, 0);
        }
        let mut published = 0;
        for signal in signals {
            if sender.send(signal).is_ok() {
                published += 1;
            }
        }
        (subscribers, published)
    }

    fn subscribe(&self, account: &str) -> Result<broadcast::Receiver<Signal>> {
        let mut accounts = self
            .accounts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let sender = accounts
            .entry(account.to_owned())
            .or_insert_with(|| broadcast::channel(NOTICE_CAPACITY).0);
        if sender.receiver_count() >= STREAMS_PER_ACCOUNT {
            return Err(Error::RateLimited(5));
        }
        Ok(sender.subscribe())
    }

    fn release(&self, account: &str) {
        let mut accounts = self
            .accounts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if accounts
            .get(account)
            .is_some_and(|sender| sender.receiver_count() == 0)
        {
            accounts.remove(account);
        }
    }
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
        actor.check_session(&self.store).await?;
        let receiver = self.hub.subscribe(actor.id())?;
        Ok(ChangeStream::new(self.clone(), actor.clone(), receiver))
    }
}
