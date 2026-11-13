use super::AlbumEditionTrackListAvailability;
use crate::{Actor, ArtistCatalog, Error, Result};
use hito_plugin_sdk::CatalogTrackKind;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, future::Future};
use utoipa::ToSchema;

const MAX_IDENTITY_BYTES: usize = 256;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadAlbumLibraryLevel {
    pub release_group_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryLevel {
    InLibrary,
    Partly,
    Linked,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AlbumLibraryLevel {
    Compared {
        level: LibraryLevel,
        matched_tracks: u32,
        edition_tracks: u32,
    },
    NeedsChoice,
    Disabled,
    Unsupported,
    Unavailable,
    Cancelled,
}

impl AlbumLibraryLevel {
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Compared { .. } => "compared",
            Self::NeedsChoice => "needs_choice",
            Self::Disabled => "disabled",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
        }
    }
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "album_library_level", request_id = %uuid::Uuid::new_v4()))]
    pub async fn album_library_level_until(
        &self,
        actor: &Actor,
        release: &str,
        request: ReadAlbumLibraryLevel,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AlbumLibraryLevel> {
        if request.release_group_id.is_empty()
            || request.release_group_id.len() > MAX_IDENTITY_BYTES
        {
            return Err(Error::Invalid(
                "Use a bounded release group identity".into(),
            ));
        }
        let read = self
            .bounded_album_read(
                cancelled,
                self.read_saved_album_edition(actor, release, Some(&request.release_group_id)),
            )
            .await?;
        let level = classify(read);
        tracing::debug!(
            outcome = level.outcome(),
            "Album library level read completed"
        );
        Ok(level)
    }
}

fn classify(read: AlbumEditionTrackListAvailability) -> AlbumLibraryLevel {
    match read {
        AlbumEditionTrackListAvailability::Available {
            edition, matches, ..
        } => {
            let matched: HashSet<&str> = matches
                .iter()
                .map(|found| found.occurrence_id.as_str())
                .collect();
            let audio = edition
                .media
                .iter()
                .flat_map(|medium| &medium.tracks)
                .filter(|track| track.kind == CatalogTrackKind::Audio);
            let (total, found) = audio.fold((0_u32, 0_u32), |(total, found), track| {
                (
                    total + 1,
                    found + u32::from(matched.contains(track.id.as_str())),
                )
            });
            let level = if total > 0 && found == total {
                LibraryLevel::InLibrary
            } else if found > 0 {
                LibraryLevel::Partly
            } else {
                LibraryLevel::Linked
            };
            AlbumLibraryLevel::Compared {
                level,
                matched_tracks: found,
                edition_tracks: total,
            }
        }
        AlbumEditionTrackListAvailability::NeedsChoice => AlbumLibraryLevel::NeedsChoice,
        AlbumEditionTrackListAvailability::Disabled => AlbumLibraryLevel::Disabled,
        AlbumEditionTrackListAvailability::Unsupported => AlbumLibraryLevel::Unsupported,
        AlbumEditionTrackListAvailability::Unavailable => AlbumLibraryLevel::Unavailable,
        AlbumEditionTrackListAvailability::Cancelled => AlbumLibraryLevel::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hito_plugin_sdk::{CatalogEditionMedium, CatalogEditionTrackList, CatalogTrackOccurrence};

    fn read(kinds: &[CatalogTrackKind]) -> AlbumEditionTrackListAvailability {
        let edition = Box::new(CatalogEditionTrackList {
            id: "edition".into(),
            release_group_id: "group".into(),
            title: "Edition".into(),
            artist_credit: None,
            revision: None,
            media: vec![CatalogEditionMedium {
                position: 1,
                title: None,
                format: None,
                tracks: kinds
                    .iter()
                    .enumerate()
                    .map(|(index, kind)| CatalogTrackOccurrence {
                        id: format!("occurrence-{index}"),
                        position: index as u32,
                        number: None,
                        kind: *kind,
                        title: "Track".into(),
                        artist_credit: None,
                        duration_ms: None,
                        recording_id: None,
                    })
                    .collect(),
            }],
        });
        AlbumEditionTrackListAvailability::Available {
            association_id: "association".into(),
            edition_digest: super::super::editions::edition_digest(&edition).unwrap(),
            stats: crate::EditionTrackStats::from_edition(&edition),
            edition,
            matches: Vec::new(),
            library_only: Vec::new(),
            attached_occurrences: Vec::new(),
            adoption: None,
            is_album_release: false,
        }
    }

    #[test]
    fn an_edition_without_a_matching_audio_track_is_only_linked() {
        let AlbumLibraryLevel::Compared {
            level,
            matched_tracks,
            edition_tracks,
        } = classify(read(&[
            CatalogTrackKind::Pregap,
            CatalogTrackKind::Audio,
            CatalogTrackKind::Audio,
            CatalogTrackKind::Data,
        ]))
        else {
            panic!("an available edition compares");
        };
        assert!(level == LibraryLevel::Linked);
        assert_eq!((matched_tracks, edition_tracks), (0, 2));
    }

    #[test]
    fn an_edition_without_audio_tracks_is_never_in_the_library() {
        let AlbumLibraryLevel::Compared { level, .. } = classify(read(&[CatalogTrackKind::Data]))
        else {
            panic!("an available edition compares");
        };
        assert!(level == LibraryLevel::Linked);
    }
}
