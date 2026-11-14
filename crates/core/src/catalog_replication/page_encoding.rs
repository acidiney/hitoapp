use super::CatalogSnapshotPage;
use serde::{Serialize, Serializer, ser::SerializeStruct};
use serde_json::value::RawValue;

struct EncodedPage<'a> {
    page: &'a CatalogSnapshotPage,
    recordings: &'a [Box<RawValue>],
}

impl Serialize for EncodedPage<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let CatalogSnapshotPage {
            snapshot_id,
            ordinal,
            recordings: _,
            next_ordinal,
        } = self.page;
        let mut state = serializer.serialize_struct("CatalogSnapshotPage", 4)?;
        state.serialize_field("snapshot_id", snapshot_id)?;
        state.serialize_field("ordinal", ordinal)?;
        state.serialize_field("recordings", self.recordings)?;
        state.serialize_field("next_ordinal", next_ordinal)?;
        state.end()
    }
}

pub(super) fn encode(
    page: &CatalogSnapshotPage,
    recordings: &[Box<RawValue>],
) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&EncodedPage { page, recordings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog_replication::{
        LocalRecordingFact, LocalReleaseAppearance, LocalSourceFact, ServerRelayObservation,
    };
    use uuid::Uuid;

    fn fact(index: usize) -> LocalRecordingFact {
        let unusual = index == 0;
        LocalRecordingFact {
            id: if unusual {
                "id\"\\\n\u{0001}é🎵".into()
            } else {
                format!("recording-{index}")
            },
            library_id: "library-雪".into(),
            title: if unusual {
                "title\t\r\0\u{001f} \u{2028}".into()
            } else {
                format!("title-{index}")
            },
            artist: if unusual {
                "artist 東京".into()
            } else {
                "artist".into()
            },
            duration_ms: [-1, 0, 86_400_000][index % 3],
            sources: vec![
                LocalSourceFact {
                    id: format!("source-{index}-ready"),
                    observation_id: unusual.then(|| "observe-ø".into()),
                    title: "source title".into(),
                    artist: "source artist".into(),
                    duration_ms: 9_223_372_036_854_775_000,
                    server_relay: ServerRelayObservation::Ready,
                    release: Some(LocalReleaseAppearance {
                        id: "release-id".into(),
                        entry_id: "entry-id".into(),
                        title: "release title".into(),
                        artist: None,
                        disc_number: Some(-1),
                        track_number: Some(0),
                    }),
                },
                LocalSourceFact {
                    id: format!("source-{index}-unavailable"),
                    observation_id: None,
                    title: "other source".into(),
                    artist: "other artist".into(),
                    duration_ms: 0,
                    server_relay: ServerRelayObservation::Unavailable,
                    release: None,
                },
            ],
        }
    }

    fn assert_equivalent(ordinal: u64, next_ordinal: Option<u64>, count: usize) {
        let recordings = (0..count).map(fact).collect::<Vec<_>>();
        let raw = recordings
            .iter()
            .map(serde_json::value::to_raw_value)
            .collect::<serde_json::Result<Vec<_>>>()
            .unwrap();
        let page = CatalogSnapshotPage {
            snapshot_id: Uuid::from_u128(0x1020_3040_5060_7080_90a0_b0c0_d0e0_f001),
            ordinal,
            recordings,
            next_ordinal,
        };
        let candidate = encode(&page, &raw).unwrap();
        let expected = serde_json::to_vec(&page).unwrap();
        assert_eq!(candidate, expected);
        let decoded: CatalogSnapshotPage = serde_json::from_slice(&candidate).unwrap();
        assert_eq!(decoded.snapshot_id, page.snapshot_id);
        assert_eq!(decoded.ordinal, page.ordinal);
        assert_eq!(decoded.next_ordinal, page.next_ordinal);
        assert_eq!(decoded.recordings.len(), page.recordings.len());
        for (actual, expected) in decoded.recordings.iter().zip(&page.recordings) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.library_id, expected.library_id);
            assert_eq!(actual.title, expected.title);
            assert_eq!(actual.artist, expected.artist);
            assert_eq!(actual.duration_ms, expected.duration_ms);
            assert_eq!(actual.sources.len(), expected.sources.len());
            for (actual, expected) in actual.sources.iter().zip(&expected.sources) {
                assert_eq!(actual.id, expected.id);
                assert_eq!(actual.observation_id, expected.observation_id);
                assert_eq!(actual.title, expected.title);
                assert_eq!(actual.artist, expected.artist);
                assert_eq!(actual.duration_ms, expected.duration_ms);
                assert_eq!(actual.server_relay, expected.server_relay);
                assert_eq!(
                    actual.release.as_ref().map(|release| (
                        release.id.as_str(),
                        release.entry_id.as_str(),
                        release.title.as_str(),
                        release.artist.as_deref(),
                        release.disc_number,
                        release.track_number,
                    )),
                    expected.release.as_ref().map(|release| (
                        release.id.as_str(),
                        release.entry_id.as_str(),
                        release.title.as_str(),
                        release.artist.as_deref(),
                        release.disc_number,
                        release.track_number,
                    )),
                );
            }
        }
        let reencoded = serde_json::to_vec(&decoded).unwrap();
        assert_eq!(reencoded, expected);
    }

    #[test]
    fn cached_page_encoding_matches_typed_page_bytes() {
        assert_equivalent(0, None, 0);
        assert_equivalent(1, Some(2), 2);
        assert_equivalent(2, None, 3);
        assert_equivalent(9, Some(10), 200);
    }
}
