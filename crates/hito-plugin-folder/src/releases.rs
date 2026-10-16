use crate::catalog::Track;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn assign(tracks: &mut BTreeMap<String, Track>) {
    let mut groups: BTreeMap<(usize, String, String), Vec<String>> = BTreeMap::new();
    for track in tracks.values() {
        if let Some(album) = &track.album {
            let directory = Arc::as_ptr(&track.directory) as usize;
            groups
                .entry((directory, album.clone(), track.album_artist.clone()))
                .or_default()
                .push(track.id.clone());
        }
    }
    for ((_, album, artist), ids) in groups {
        let evidence = serde_json::to_vec(&(album, artist, &ids))
            .expect("Release identity contains only text");
        let release = format!("release-{:x}", Sha256::digest(evidence));
        for id in ids {
            if let Some(track) = tracks.get_mut(&id) {
                track.release_id = Some(release.clone());
            }
        }
    }
}
