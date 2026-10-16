use crate::catalog::Track;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) fn assign(tracks: &mut [Track]) {
    let mut groups: BTreeMap<(usize, String, String), Vec<usize>> = BTreeMap::new();
    for (index, track) in tracks.iter().enumerate() {
        if let Some(album) = &track.album {
            let directory = Arc::as_ptr(&track.directory) as usize;
            groups
                .entry((directory, album.clone(), track.album_artist.clone()))
                .or_default()
                .push(index);
        }
    }
    for ((_, album, artist), indexes) in groups {
        let ids: BTreeSet<_> = indexes
            .iter()
            .map(|index| tracks[*index].id.clone())
            .collect();
        let evidence = serde_json::to_vec(&(album, artist, &ids))
            .expect("Release identity contains only text");
        let release = format!("release-{:x}", Sha256::digest(evidence));
        for index in indexes {
            tracks[index].release_id = Some(release.clone());
        }
    }
}
