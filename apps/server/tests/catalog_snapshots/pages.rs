use super::fixture::{Fixture, ROOT, tag_wav};
use axum::http::StatusCode;
use hito_core::{CatalogSnapshotPage, test_support::support::write_wav};
use serde_json::Value;
use std::collections::BTreeSet;

#[tokio::test]
async fn escaped_local_metadata_crosses_byte_page_boundaries_without_a_wire_envelope() {
    let f = Fixture::start().await;
    let mut titles = BTreeSet::new();
    for index in 0..47 {
        let path = f.music.join(format!("{index:03}.wav"));
        write_wav(&path, index);
        let title = format!("Owned {index:03} {}", "\"\\\n\t".repeat(700));
        tag_wav(&path, &title);
        titles.insert(title);
    }
    f.scan().await;
    let snapshot = f.begin().await;
    assert_eq!(snapshot["recording_count"], 47);
    assert!(snapshot["page_count"].as_u64().unwrap() > 1);
    let id = snapshot["id"].as_str().unwrap();
    let mut seen = BTreeSet::new();
    let mut actual_titles = BTreeSet::new();
    let mut sizes = Vec::new();
    for ordinal in 0..snapshot["page_count"].as_u64().unwrap() {
        let reply = f
            .call("GET", &format!("{ROOT}/{id}/pages/{ordinal}"), Value::Null)
            .await;
        assert_eq!(reply.status, StatusCode::OK);
        reply.private();
        let page: CatalogSnapshotPage = serde_json::from_slice(&reply.bytes).unwrap();
        assert_eq!(reply.bytes, serde_json::to_vec(&page).unwrap());
        assert!(reply.bytes.len() <= 262_144);
        sizes.push(reply.bytes.len());
        assert_eq!(page.ordinal, ordinal);
        assert_eq!(page.snapshot_id.to_string(), id);
        assert_eq!(
            page.next_ordinal,
            (ordinal + 1 < snapshot["page_count"].as_u64().unwrap()).then_some(ordinal + 1)
        );
        for recording in page.recordings {
            assert!(seen.insert(recording.id));
            assert_eq!(recording.sources.len(), 1);
            assert_eq!(recording.sources[0].title, recording.title);
            assert!(actual_titles.insert(recording.title));
        }
    }
    assert_eq!(seen.len(), 47);
    assert_eq!(actual_titles, titles);
    assert!(
        sizes.iter().any(|size| *size > 240_000),
        "The owned metadata must exercise a byte boundary close to the page limit"
    );
    f.services.catalog_replication.close().await.unwrap();
}
