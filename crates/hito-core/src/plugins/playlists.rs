use super::transport::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    MAX_PLAYLIST_ITEMS, MAX_PLAYLIST_PAGE_ITEMS, PLAYLIST_DETAIL_PATH, PLAYLIST_ITEMS_PATH,
    PlaylistDetailRequest, PlaylistItemsRequest, PlaylistPage, ProviderPlaylist,
    ProviderPlaylistEntry, ProviderPlaylistItem,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

const SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGES: usize = MAX_PLAYLIST_ITEMS / MAX_PLAYLIST_PAGE_ITEMS as usize;

pub(crate) struct PlaylistSnapshot {
    pub(crate) detail: ProviderPlaylist,
    pub(crate) entries: Vec<ProviderPlaylistEntry>,
    pub(crate) content_digest: String,
}

struct SnapshotDigest(Sha256);

impl std::io::Write for SnapshotDigest {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl ControlClient {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn playlist_snapshot(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        reference: String,
    ) -> Result<PlaylistSnapshot> {
        self.playlist_snapshot_checked(
            account,
            base_url,
            token,
            PlaylistDetailRequest::Reference { reference },
            || async { Ok(()) },
        )
        .await
    }

    pub(super) async fn playlist_snapshot_checked<F, Check>(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: PlaylistDetailRequest,
        check: F,
    ) -> Result<PlaylistSnapshot>
    where
        F: Fn() -> Check + Send + Sync,
        Check: std::future::Future<Output = Result<()>> + Send,
    {
        let _admission = self.admit(account)?;
        self.playlist_snapshot_admitted(base_url, token, request, check)
            .await
    }

    pub(super) async fn follow_playlist_snapshot<F, Check>(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: PlaylistDetailRequest,
        check: F,
    ) -> Result<PlaylistSnapshot>
    where
        F: Fn() -> Check + Send + Sync,
        Check: std::future::Future<Output = Result<()>> + Send,
    {
        let _admission = self.admit_follow(account)?;
        self.playlist_snapshot_admitted(base_url, token, request, check)
            .await
    }

    async fn playlist_snapshot_admitted<F, Check>(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: PlaylistDetailRequest,
        check: F,
    ) -> Result<PlaylistSnapshot>
    where
        F: Fn() -> Check + Send + Sync,
        Check: std::future::Future<Output = Result<()>> + Send,
    {
        tokio::time::timeout(
            Duration::from_secs(60),
            self.read_playlist(base_url, token, request, check),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "The provider playlist exceeded the time limit",
        )))
    }

    async fn read_playlist<F, Check>(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: PlaylistDetailRequest,
        check: F,
    ) -> Result<PlaylistSnapshot>
    where
        F: Fn() -> Check + Send + Sync,
        Check: std::future::Future<Output = Result<()>> + Send,
    {
        let mut remaining = SNAPSHOT_BYTES;
        check().await?;
        let detail = self
            .playlist_detail(base_url, token, request, &mut remaining)
            .await?;
        let mut entries = Vec::with_capacity(detail.total_count as usize);
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut occurrences = HashSet::new();
        let mut recordings = HashMap::new();
        let mut complete = false;
        for _ in 0..MAX_PAGES {
            check().await?;
            let request = PlaylistItemsRequest {
                playlist_id: detail.id.clone(),
                snapshot: detail.snapshot.clone(),
                cursor,
                limit: MAX_PLAYLIST_PAGE_ITEMS,
            };
            let page: PlaylistPage = self
                .exchange_admitted(
                    base_url,
                    token,
                    Call {
                        path: PLAYLIST_ITEMS_PATH,
                        body: Some(serde_json::to_value(&request).map_err(|_| {
                            Error::Internal("Playlist request could not be encoded".into())
                        })?),
                        maximum: 1024 * 1024,
                    },
                    &mut remaining,
                )
                .await?;
            page.validate(request.limit)
                .map_err(|_| Error::Upstream("The provider returned invalid playlist items"))?;
            if page.playlist_id != detail.id || page.snapshot != detail.snapshot {
                return Err(Error::Upstream(
                    "The provider playlist changed during import",
                ));
            }
            if page.items.len() > (detail.total_count as usize).saturating_sub(entries.len()) {
                return Err(Error::Upstream(
                    "The provider returned an inconsistent playlist count",
                ));
            }
            for entry in &page.items {
                if entry
                    .occurrence_id
                    .as_ref()
                    .is_some_and(|id| !occurrences.insert(id.clone()))
                {
                    return Err(Error::Upstream(
                        "The provider repeated a playlist occurrence ID",
                    ));
                }
                if let ProviderPlaylistItem::Recording { recording } = &entry.item
                    && let Some(previous) =
                        recordings.insert(recording.id.clone(), recording.clone())
                    && previous != *recording
                {
                    return Err(Error::Upstream(
                        "The provider returned inconsistent recording metadata",
                    ));
                }
            }
            let empty = page.items.is_empty();
            entries.extend(page.items);
            match page.next_cursor {
                Some(next) => {
                    if empty
                        || entries.len() == detail.total_count as usize
                        || !cursors.insert(next.clone())
                    {
                        return Err(Error::Upstream(
                            "The provider returned an incomplete playlist",
                        ));
                    }
                    cursor = Some(next);
                }
                None => {
                    complete = entries.len() == detail.total_count as usize;
                    break;
                }
            }
        }
        if !complete {
            return Err(Error::Upstream(
                "The provider returned an incomplete playlist",
            ));
        }
        check().await?;
        let final_detail = self
            .playlist_detail(
                base_url,
                token,
                PlaylistDetailRequest::Canonical {
                    playlist_id: detail.id.clone(),
                },
                &mut remaining,
            )
            .await?;
        if final_detail != detail {
            return Err(Error::Upstream(
                "The provider playlist changed during import",
            ));
        }
        let mut digest = SnapshotDigest(Sha256::new());
        serde_json::to_writer(
            &mut digest,
            &("hito-playlist-snapshot:v1", &detail, &entries),
        )
        .map_err(|_| {
            Error::Internal("Provider playlist snapshot could not be fingerprinted".into())
        })?;
        let content_digest = format!("{:x}", digest.0.finalize());
        tracing::debug!(
            entries = entries.len(),
            bytes = SNAPSHOT_BYTES - remaining,
            "Provider playlist snapshot completed"
        );
        Ok(PlaylistSnapshot {
            detail,
            entries,
            content_digest,
        })
    }

    async fn playlist_detail(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: PlaylistDetailRequest,
        remaining: &mut usize,
    ) -> Result<ProviderPlaylist> {
        request
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let detail: ProviderPlaylist = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: PLAYLIST_DETAIL_PATH,
                    body: Some(serde_json::to_value(&request).map_err(|_| {
                        Error::Internal("Playlist request could not be encoded".into())
                    })?),
                    maximum: 32768,
                },
                remaining,
            )
            .await?;
        detail
            .validate()
            .map_err(|_| Error::Upstream("The provider returned invalid playlist details"))?;
        Ok(detail)
    }
}
