use hito_core::{
    CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, LocalRecordingFact,
    LocalReleaseAppearance, LocalSourceFact, ServerRelayObservation,
};
use uuid::Uuid;

pub const ACCOUNT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const LIBRARY: &str = "owned-remote-library";
pub const PAGE_SIZE: usize = 200;
pub const SNAPSHOT: Uuid = Uuid::from_u128(701);

#[derive(Clone, Copy)]
pub struct Export {
    pub id: Uuid,
    pub recordings: usize,
}

impl Export {
    pub fn metadata(self) -> CatalogSnapshot {
        CatalogSnapshot {
            id: self.id,
            account_id: ACCOUNT.into(),
            library_id: LIBRARY.into(),
            mode: CatalogSnapshotMode::SnapshotOnly,
            collection_watermark: 91_337,
            recording_count: self.recordings,
            page_count: self.recordings.div_ceil(PAGE_SIZE).max(1) as u64,
            expires_at: 1,
        }
    }

    pub fn page(self, ordinal: u64) -> CatalogSnapshotPage {
        let start = ordinal as usize * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.recordings);
        CatalogSnapshotPage {
            snapshot_id: self.id,
            ordinal,
            recordings: (start..end).map(|index| self.fact(index)).collect(),
            next_ordinal: (ordinal + 1 < self.metadata().page_count).then_some(ordinal + 1),
        }
    }

    fn fact(self, index: usize) -> LocalRecordingFact {
        let identity = |kind: &str| {
            if index == 0 {
                String::new()
            } else {
                format!("legacy {kind}:{}:{index}", self.id)
            }
        };
        LocalRecordingFact {
            id: identity("recording"),
            library_id: LIBRARY.into(),
            title: format!("Inherited track {index}"),
            artist: "Inherited artist".into(),
            duration_ms: 1000 + index as i64,
            sources: vec![LocalSourceFact {
                id: identity("source"),
                observation_id: Some(identity("observation")),
                title: format!("Source track {index}"),
                artist: "Source artist".into(),
                duration_ms: 999 + index as i64,
                server_relay: if index == 0 {
                    ServerRelayObservation::Ready
                } else {
                    ServerRelayObservation::Unavailable
                },
                release: Some(LocalReleaseAppearance {
                    id: identity("release"),
                    entry_id: identity("entry"),
                    title: format!("Source release {index}"),
                    artist: Some("Release artist".into()),
                    disc_number: Some(1 + index as i64 % 3),
                    track_number: Some(1 + index as i64 % 17),
                }),
            }],
        }
    }
}
