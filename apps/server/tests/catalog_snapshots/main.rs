mod authority;
mod disconnect;
mod expiry;
mod fixture;
mod input;
mod lifecycle;
mod origin;
mod pages;

use axum::http::StatusCode;
use fixture::{Fixture, ROOT};
use hito_core::{
    CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, ServerRelayObservation,
    test_support::support::write_wav,
};
use serde_json::Value;

#[tokio::test]
async fn an_owned_local_recording_roundtrips_through_the_snapshot_routes() {
    let f = Fixture::start().await;
    let original = write_wav(&f.music.join("Owned.wav"), 0);
    f.scan().await;
    let before = f
        .call("GET", "/api/v1/sync?cursor=0", Value::Null)
        .await
        .json();
    let metadata = f.begin().await;
    let snapshot: CatalogSnapshot = serde_json::from_value(metadata).unwrap();
    assert_eq!(snapshot.account_id, f.account);
    assert_eq!(snapshot.library_id, f.library);
    assert_eq!(snapshot.mode, CatalogSnapshotMode::SnapshotOnly);
    assert_eq!(snapshot.recording_count, 1);
    assert_eq!(snapshot.page_count, 1);
    let reply = f
        .call(
            "GET",
            &format!("{ROOT}/{}/pages/0", snapshot.id),
            Value::Null,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.private();
    let page: CatalogSnapshotPage = serde_json::from_slice(&reply.bytes).unwrap();
    assert_eq!(reply.bytes, serde_json::to_vec(&page).unwrap());
    assert!(reply.bytes.len() <= 262_144);
    assert_eq!(page.snapshot_id, snapshot.id);
    assert_eq!(page.ordinal, 0);
    assert_eq!(page.next_ordinal, None);
    assert_eq!(page.recordings.len(), 1);
    let fact = &page.recordings[0];
    assert_eq!(fact.library_id, f.library);
    assert_eq!(fact.title, "Owned");
    assert_eq!(fact.duration_ms, 1_000);
    assert_eq!(fact.sources.len(), 1);
    assert_eq!(fact.sources[0].server_relay, ServerRelayObservation::Ready);
    let encoded = std::str::from_utf8(&reply.bytes).unwrap();
    for private in [
        f.directory.path().to_str().unwrap(),
        "snapshot-http-host",
        "relative_path",
        "file_checksum",
        "device_id",
        "credential",
        "https://",
    ] {
        assert!(!encoded.contains(private));
    }
    assert_eq!(
        f.call("GET", "/api/v1/sync?cursor=0", Value::Null)
            .await
            .json(),
        before
    );
    assert_eq!(std::fs::read(f.music.join("Owned.wav")).unwrap(), original);
    f.services.catalog_replication.close().await.unwrap();
}
