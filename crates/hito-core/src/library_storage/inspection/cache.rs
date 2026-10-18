use super::snapshot::Snapshot;
use crate::library_storage::types::{
    AUDIO_ALGORITHM, AudioEvidence, IndexedFile, METADATA_VERSION,
};

#[derive(sqlx::FromRow)]
pub(crate) struct CachedFile {
    pub checksum: String,
    pub audio_digest: Option<String>,
    pub metadata_version: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration_ms: i64,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
    pub tag_details: Option<String>,
}

impl CachedFile {
    pub(super) fn metadata_current(&self) -> bool {
        self.metadata_version == METADATA_VERSION
    }

    pub(super) fn audio(&self) -> Option<AudioEvidence> {
        self.audio_digest.as_ref().map(|digest| AudioEvidence {
            algorithm: AUDIO_ALGORITHM,
            digest: digest.clone(),
        })
    }

    pub(super) fn into_indexed(self, snapshot: Snapshot) -> IndexedFile {
        IndexedFile {
            path: snapshot.path,
            checksum: snapshot.checksum,
            size: snapshot.size,
            audio: self.audio_digest.map(|digest| AudioEvidence {
                algorithm: AUDIO_ALGORITHM,
                digest,
            }),
            title: self.title,
            artist: self.artist,
            album: self.album,
            album_artist: self.album_artist,
            duration_ms: self.duration_ms,
            disc_number: self.disc_number,
            track_number: self.track_number,
            tag_details: self.tag_details,
        }
    }
}
