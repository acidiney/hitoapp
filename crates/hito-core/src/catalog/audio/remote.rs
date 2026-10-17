use crate::{Error, PlaybackSource, Result};
use bytes::Bytes;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit, watch};
use tracing::{Instrument, instrument::WithSubscriber};

#[derive(Clone)]
pub(crate) enum Authority {
    Provider(Box<crate::plugins::AudioAuthority>),
    Podcast(Box<PlaybackSource>),
    Download(crate::downloads::DownloadAuthority),
}

impl Authority {
    async fn check(&self) -> Result<()> {
        match self {
            Self::Provider(authority) => authority.check().await,
            Self::Podcast(authority) => authority.check().await,
            Self::Download(authority) => authority.check().await,
        }
    }
}

pub(crate) struct Permits {
    pub _account: OwnedSemaphorePermit,
    pub _global: OwnedSemaphorePermit,
}

struct Connection {
    response: Option<reqwest::Response>,
    permits: Option<Permits>,
    pending: Bytes,
}

pub(crate) struct RemoteAudio {
    connection: Arc<Mutex<Connection>>,
    authority: Authority,
    revoked: watch::Receiver<bool>,
    monitor: tokio::task::JoinHandle<()>,
}
impl RemoteAudio {
    pub(crate) fn new(
        response: reqwest::Response,
        authority: Authority,
        permits: Option<Permits>,
    ) -> Self {
        let connection = Arc::new(Mutex::new(Connection {
            response: Some(response),
            permits,
            pending: Bytes::new(),
        }));
        let weak = Arc::downgrade(&connection);
        let (cancel, revoked) = watch::channel(false);
        let guard = authority.clone();
        let monitor = tokio::spawn(
            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    if guard.check().await.is_err() {
                        let _ = cancel.send(true);
                        if let Some(connection) = weak.upgrade() {
                            let mut connection = connection.lock().await;
                            connection.response = None;
                            connection.permits = None;
                            connection.pending = Bytes::new();
                        }
                        tracing::debug!(
                            event = "remote_audio_authority_withdrawn",
                            "Remote audio authority withdrawn"
                        );
                        break;
                    }
                }
            }
            .instrument(tracing::Span::current())
            .with_current_subscriber(),
        );
        Self {
            connection,
            authority,
            revoked,
            monitor,
        }
    }
    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>> {
        self.authority.check().await?;
        let mut connection = self.connection.lock().await;
        if *self.revoked.borrow() {
            return Err(Error::NotFound);
        }
        let result = if connection.pending.is_empty() {
            let response = connection.response.as_mut().ok_or(Error::NotFound)?;
            tokio::select! {
                biased;
                _ = self.revoked.changed() => Err(Error::NotFound),
                result = tokio::time::timeout(Duration::from_secs(15), response.chunk()) => {
                    match result {
                        Ok(chunk) => chunk.map_err(|_| Error::Upstream("The audio stream could not be read")),
                        Err(_) => Err(Error::Upstream("The audio stream did not return data in time")),
                    }
                },
            }
        } else {
            Ok(Some(std::mem::take(&mut connection.pending)))
        };
        self.authority.check().await?;
        result.map(|chunk| {
            chunk.map(|mut bytes| {
                if bytes.len() > 65536 {
                    connection.pending = bytes.split_off(65536);
                }
                bytes
            })
        })
    }
}
impl Drop for RemoteAudio {
    fn drop(&mut self) {
        self.monitor.abort();
    }
}
