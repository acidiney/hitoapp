use super::snapshot::Snapshot;
use crate::library_storage::types::{AUDIO_ALGORITHM, AudioEvidence, IndexedFile};

#[derive(sqlx::FromRow)]
pub(crate) struct CachedFile {
    pub checksum: String,
    pub audio_digest: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration_ms: i64,
}

impl CachedFile {
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
        }
    }
}
