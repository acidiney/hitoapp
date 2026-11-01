mod artwork;
mod credits;
mod model;
mod session;
mod snapshot;
mod transport;

use crate::{Config, InitError, error::Error};
use hito_plugin_sdk::{
    MAX_PLAYLIST_ITEMS, PlaylistAttribution, PlaylistDetailRequest, PlaylistItemsRequest,
    PlaylistPage, ProviderPlaylist,
};
use snapshot::{Cache, Snapshot};
use std::sync::Arc;
use tokio::sync::Semaphore;
use transport::Transport;

pub(crate) struct Client {
    transport: Transport,
    slots: Arc<Semaphore>,
    artwork_slots: Arc<Semaphore>,
    cache: Cache,
}

impl Client {
    pub(crate) fn new(config: &Config) -> Result<Self, InitError> {
        Ok(Self {
            transport: Transport::new(config)?,
            slots: Arc::new(Semaphore::new(4)),
            artwork_slots: Arc::new(Semaphore::new(4)),
            cache: Cache::default(),
        })
    }

    pub(crate) async fn read(
        &self,
        request: PlaylistDetailRequest,
    ) -> Result<ProviderPlaylist, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let id = match request {
            PlaylistDetailRequest::Canonical { playlist_id } => playlist_id,
            PlaylistDetailRequest::Reference { reference } => reference_id(&reference)?,
        };
        if !model::valid_id(&id) {
            return Err(Error::InvalidRequest);
        }
        let _permit = self.slots.try_acquire().map_err(|_| Error::Busy)?;
        let snapshot = self.fetch(&id).await?;
        let playlist = snapshot.playlist.clone();
        self.cache.insert(snapshot)?;
        tracing::debug!(
            event = "spotify_playlist_normalized",
            outcome = "ready",
            item_count = playlist.total_count
        );
        Ok(playlist)
    }

    pub(crate) fn items(&self, request: PlaylistItemsRequest) -> Result<PlaylistPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        if !model::valid_id(&request.playlist_id) || !snapshot::valid_digest(&request.snapshot) {
            return Err(Error::InvalidRequest);
        }
        let _permit = self.slots.try_acquire().map_err(|_| Error::Busy)?;
        self.cache.page(request)
    }

    async fn fetch(&self, id: &str) -> Result<Snapshot, Error> {
        let bootstrap = self.transport.bootstrap(id).await?;
        let session = session::read(&bootstrap)?;
        drop(bootstrap);
        let mut offset = 0;
        let mut name = None;
        let mut total = None;
        let mut items = Vec::new();
        let mut normalized_bytes =
            id.len() + 64 + "Spotify".len() + 1024 + std::mem::size_of::<Snapshot>();
        for page_number in 0..100 {
            session.validate_expiration()?;
            let bytes = self.transport.page(id, &session.token, offset).await?;
            let page = model::read(&bytes, id)?;
            if name.as_ref().is_some_and(|name| *name != page.name)
                || total.is_some_and(|total| total != page.total)
                || page.total > MAX_PLAYLIST_ITEMS
                || page.items.len() > 100
                || items.len() + page.items.len() > page.total
            {
                return Err(Error::InvalidResponse);
            }
            if name.is_none() {
                normalized_bytes = normalized_bytes
                    .checked_add(page.name.len())
                    .and_then(|size| {
                        size.checked_add(
                            page.total
                                * std::mem::size_of::<hito_plugin_sdk::ProviderPlaylistEntry>(),
                        )
                    })
                    .filter(|size| *size <= snapshot::MAX_NORMALIZED_BYTES)
                    .ok_or(Error::InvalidResponse)?;
                items.reserve_exact(page.total);
                name = Some(page.name);
                total = Some(page.total);
            }
            for entry in page.items {
                let encoded = serde_json::to_vec(&entry).map_err(|_| Error::InvalidResponse)?;
                normalized_bytes = normalized_bytes
                    .checked_add(encoded.len())
                    .filter(|size| *size <= snapshot::MAX_NORMALIZED_BYTES)
                    .ok_or(Error::InvalidResponse)?;
                items.push(entry);
            }
            tracing::debug!(
                event = "spotify_public_page_normalized",
                page_count = page_number + 1,
                item_count = items.len()
            );
            match page.next_offset {
                Some(next) if next == items.len() && next > offset && next < page.total => {
                    offset = next;
                }
                None if items.len() == page.total => {
                    let name = name.ok_or(Error::InvalidResponse)?;
                    let snapshot = snapshot::digest(id, &name, &items)?;
                    let playlist = ProviderPlaylist {
                        id: id.to_owned(),
                        name,
                        snapshot,
                        total_count: page.total as u32,
                        attribution: PlaylistAttribution {
                            label: "Spotify".into(),
                        },
                    };
                    playlist.validate().map_err(|_| Error::InvalidResponse)?;
                    return Ok(Snapshot {
                        playlist,
                        items: items.into(),
                        charged_bytes: normalized_bytes,
                    });
                }
                _ => return Err(Error::InvalidResponse),
            }
        }
        Err(Error::InvalidResponse)
    }
}

fn reference_id(reference: &str) -> Result<String, Error> {
    if reference.contains('#') {
        return Err(Error::InvalidRequest);
    }
    let path = reference
        .strip_prefix("https://open.spotify.com/playlist/")
        .ok_or(Error::InvalidRequest)?;
    let id = path.split_once('?').map_or(path, |(id, _)| id);
    if !model::valid_id(id) {
        return Err(Error::InvalidRequest);
    }
    Ok(id.to_owned())
}
