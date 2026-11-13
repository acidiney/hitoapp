use hito_plugin_sdk::{CatalogEditionTrackList, CatalogTrackKind};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct EditionTrackStats {
    pub audio_tracks: usize,
    #[schema(required = true)]
    pub total_duration_ms: Option<u64>,
}

impl EditionTrackStats {
    pub(super) fn from_edition(edition: &CatalogEditionTrackList) -> Self {
        let mut audio = edition
            .media
            .iter()
            .flat_map(|medium| &medium.tracks)
            .filter(|track| track.kind == CatalogTrackKind::Audio);
        Self {
            audio_tracks: audio.clone().count(),
            total_duration_ms: audio
                .try_fold(0_u64, |total, track| total.checked_add(track.duration_ms?)),
        }
    }
}
