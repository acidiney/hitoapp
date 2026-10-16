use crate::catalog::Track;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::Arc,
};

pub(crate) mod state;

fn groups(tracks: &[Track]) -> Vec<Vec<usize>> {
    let mut groups = BTreeMap::<(usize, &str, &str), Vec<usize>>::new();
    for (index, track) in tracks.iter().enumerate() {
        if let Some(album) = &track.album {
            let directory = Arc::as_ptr(&track.directory) as usize;
            groups
                .entry((directory, album.as_str(), track.album_artist.as_str()))
                .or_default()
                .push(index);
        }
    }
    groups.into_values().collect()
}

pub(crate) fn assign_scan(tracks: &mut [Track]) {
    for indexes in groups(tracks) {
        let first = &tracks[indexes[0]];
        let ids: BTreeSet<_> = indexes
            .iter()
            .map(|index| tracks[*index].id.clone())
            .collect();
        let evidence =
            serde_json::to_vec(&(first.album.as_ref().unwrap(), &first.album_artist, &ids))
                .expect("Release identity contains only text");
        let release = format!("release-{:x}", Sha256::digest(evidence));
        for index in indexes {
            tracks[index].release_id = Some(release.clone());
        }
    }
}

pub(crate) async fn reconcile(
    connection: &mut SqliteConnection,
    tracks: &mut [Track],
    bootstrap: bool,
) -> io::Result<()> {
    let groups = groups(tracks);
    let prior = state::load(connection).await?;
    let mut occurrences = BTreeMap::<(String, String), BTreeSet<usize>>::new();
    let mut current = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut legacy = BTreeMap::<String, usize>::new();
    for (group, indexes) in groups.iter().enumerate() {
        let release = tracks[indexes[0]]
            .release_id
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing scanned folder release"))?;
        *legacy.entry(release.clone()).or_default() += 1;
        for index in indexes {
            let track = &tracks[*index];
            current.entry(track.id.clone()).or_default().insert(group);
            if let Some(previous) = prior.occurrences.get(&track.locator) {
                let pcm = track.evidence.as_ref().is_some_and(|evidence| {
                    previous.evidence_kind.as_deref() == Some(crate::pcm::ALGORITHM)
                        && previous.evidence.as_ref() == Some(evidence)
                });
                if previous.recording == track.id && (previous.checksum == track.checksum || pcm) {
                    occurrences
                        .entry((previous.release.clone(), track.id.clone()))
                        .or_default()
                        .insert(group);
                }
            }
        }
    }
    let mut candidates = vec![BTreeSet::<String>::new(); groups.len()];
    let mut relations = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut visits = 0usize;
    for (recording, releases) in &prior.memberships {
        for release in releases {
            let matched = occurrences
                .get(&(release.clone(), recording.clone()))
                .or_else(|| current.get(recording));
            if let Some(matched) = matched {
                for group in matched {
                    visits += 1;
                    if visits > 100000 {
                        return Err(io::Error::other(
                            "Folder release witness work limit exceeded",
                        ));
                    }
                    relations.entry(release.clone()).or_default().insert(*group);
                    candidates[*group].insert(release.clone());
                }
            }
        }
    }
    let retired = relations
        .iter()
        .filter(|(_, related)| {
            related.len() != 1 || candidates[*related.first().unwrap()].len() != 1
        })
        .map(|(release, _)| release.clone())
        .collect::<BTreeSet<_>>();
    let mut allocated = BTreeSet::new();
    let mut continued = 0;
    for (group, indexes) in groups.iter().enumerate() {
        let old = candidates[group]
            .first()
            .filter(|release| candidates[group].len() == 1 && !retired.contains(*release));
        let release = if let Some(release) = old {
            continued += 1;
            release.clone()
        } else {
            let legacy_id = tracks[indexes[0]].release_id.as_ref().unwrap();
            let release = if bootstrap && legacy.get(legacy_id) == Some(&1) {
                legacy_id.clone()
            } else {
                format!("release-{}", uuid::Uuid::new_v4())
            };
            allocated.insert(release.clone());
            release
        };
        for index in indexes {
            tracks[*index].release_id = Some(release.clone());
        }
    }
    state::allocate(connection, &allocated, &retired).await?;
    tracing::debug!(
        event = "folder_releases_resolved",
        outcome = "resolved",
        groups = groups.len(),
        continued,
        allocated = allocated.len(),
        retired = retired.len()
    );
    Ok(())
}
