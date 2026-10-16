use crate::{client::Client, error::Error};
use hito_plugin_sdk::{PlaylistAttribution, PlaylistDetailRequest, ProviderPlaylist};

pub(crate) async fn read(
    client: &Client,
    token: &str,
    request: PlaylistDetailRequest,
) -> Result<ProviderPlaylist, Error> {
    request.validate().map_err(|_| Error::InvalidRequest)?;
    let id = match request {
        PlaylistDetailRequest::Canonical { playlist_id } => playlist_id,
        PlaylistDetailRequest::Reference { reference } => reference_id(&reference)?,
    };
    if id.len() != 22 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(Error::InvalidRequest);
    }
    let metadata = client.playlist(&id, token).await?;
    if metadata.id != id {
        return Err(Error::InvalidResponse);
    }
    let items = metadata.items.ok_or(Error::AccessDenied)?;
    let playlist = ProviderPlaylist {
        id: metadata.id,
        name: metadata.name,
        snapshot: metadata.snapshot_id,
        total_count: items.total,
        attribution: PlaylistAttribution {
            label: "Spotify".into(),
        },
    };
    playlist.validate().map_err(|_| Error::InvalidResponse)?;
    tracing::debug!(
        event = "spotify_playlist_normalized",
        outcome = "ready",
        item_count = playlist.total_count
    );
    Ok(playlist)
}

fn reference_id(reference: &str) -> Result<String, Error> {
    reference
        .strip_prefix("https://open.spotify.com/playlist/")
        .map(str::to_owned)
        .ok_or(Error::InvalidRequest)
}
