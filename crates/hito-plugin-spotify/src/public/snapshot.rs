use crate::error::Error;
use hito_plugin_sdk::{
    MAX_PLAYLIST_ITEMS, PlaylistItemsRequest, PlaylistPage, ProviderPlaylist, ProviderPlaylistEntry,
};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) const MAX_NORMALIZED_BYTES: usize = 8 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 4;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const SNAPSHOT_TTL: Duration = Duration::from_secs(120);

pub(super) struct Snapshot {
    pub playlist: ProviderPlaylist,
    pub items: Arc<[ProviderPlaylistEntry]>,
    pub charged_bytes: usize,
}

struct Cached {
    snapshot: Snapshot,
    created: Instant,
}

#[derive(Default)]
pub(super) struct Cache {
    snapshots: Mutex<VecDeque<Cached>>,
}

impl Cache {
    pub(super) fn insert(&self, snapshot: Snapshot) -> Result<(), Error> {
        if snapshot.charged_bytes > MAX_NORMALIZED_BYTES {
            return Err(Error::InvalidResponse);
        }
        let now = Instant::now();
        let mut snapshots = self.snapshots.lock().map_err(|_| Error::Unavailable)?;
        snapshots.retain(|cached| {
            now.duration_since(cached.created) < SNAPSHOT_TTL
                && !(cached.snapshot.playlist.id == snapshot.playlist.id
                    && cached.snapshot.playlist.snapshot == snapshot.playlist.snapshot)
        });
        let mut bytes: usize = snapshots
            .iter()
            .map(|cached| cached.snapshot.charged_bytes)
            .sum();
        while snapshots.len() >= MAX_SNAPSHOTS || bytes + snapshot.charged_bytes > MAX_CACHE_BYTES {
            let removed = snapshots.pop_front().ok_or(Error::Unavailable)?;
            bytes -= removed.snapshot.charged_bytes;
        }
        bytes += snapshot.charged_bytes;
        let item_count = snapshot.items.len();
        snapshots.push_back(Cached {
            snapshot,
            created: now,
        });
        tracing::debug!(
            event = "spotify_public_snapshot_retained",
            snapshot_count = snapshots.len(),
            cache_bytes = bytes,
            item_count
        );
        Ok(())
    }

    pub(super) fn page(&self, request: PlaylistItemsRequest) -> Result<PlaylistPage, Error> {
        let offset = cursor(request.cursor.as_deref())?;
        let items = {
            let now = Instant::now();
            let mut snapshots = self.snapshots.lock().map_err(|_| Error::Unavailable)?;
            snapshots.retain(|cached| now.duration_since(cached.created) < SNAPSHOT_TTL);
            let cached = snapshots
                .iter()
                .find(|cached| {
                    cached.snapshot.playlist.id == request.playlist_id
                        && cached.snapshot.playlist.snapshot == request.snapshot
                })
                .ok_or(Error::SnapshotUnavailable)?;
            cached.snapshot.items.clone()
        };
        if offset > items.len() || (offset == items.len() && offset != 0) {
            return Err(Error::InvalidRequest);
        }
        let end = (offset + usize::from(request.limit)).min(items.len());
        let page = PlaylistPage {
            playlist_id: request.playlist_id,
            snapshot: request.snapshot,
            items: items[offset..end].to_vec(),
            next_cursor: (end < items.len()).then(|| end.to_string()),
        };
        page.validate(request.limit)
            .map_err(|_| Error::InvalidResponse)?;
        tracing::debug!(
            event = "spotify_public_snapshot_page",
            item_count = page.items.len(),
            has_more = page.next_cursor.is_some()
        );
        Ok(page)
    }
}

fn cursor(value: Option<&str>) -> Result<usize, Error> {
    let Some(value) = value else {
        return Ok(0);
    };
    if value.is_empty()
        || value.len() > 5
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Error::InvalidRequest);
    }
    value
        .parse::<usize>()
        .ok()
        .filter(|offset| *offset <= MAX_PLAYLIST_ITEMS)
        .ok_or(Error::InvalidRequest)
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn digest(
    id: &str,
    name: &str,
    items: &[ProviderPlaylistEntry],
) -> Result<String, Error> {
    let mut writer = HashWriter(Sha256::new());
    writer.0.update(b"hito-spotify-public-playlist-v1\0");
    serde_json::to_writer(&mut writer, &(id, name, items)).map_err(|_| Error::InvalidResponse)?;
    Ok(format!("{:x}", writer.0.finalize()))
}
