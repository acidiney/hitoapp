use hito_core::{CatalogSnapshotMode, Search, ServerRelayObservation, test_support::support};

#[tokio::test]
async fn an_indexed_local_file_exports_sanitized_source_specific_release_facts() {
    let (dir, services, actor, library, music) = support::library("private-server-host").await;
    let original = support::write_wav(&music.join("one.wav"), 0);
    support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = services
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    let snapshot = services
        .catalog_replication
        .begin(&actor, &library)
        .await
        .unwrap();
    assert_eq!(snapshot.mode, CatalogSnapshotMode::SnapshotOnly);
    assert_eq!(snapshot.account_id, actor.user().id);
    assert_eq!(snapshot.library_id, library);
    assert_eq!(snapshot.recording_count, 1);
    assert_eq!(snapshot.collection_watermark, 0);
    let page = services
        .catalog_replication
        .page(&actor, snapshot.id, 0)
        .await
        .unwrap();
    assert_eq!(page.snapshot_id, snapshot.id);
    assert_eq!(page.ordinal, 0);
    assert_eq!(page.next_ordinal, None);
    let fact = &page.recordings[0];
    assert_eq!(fact.id, recording.id);
    assert_eq!(fact.library_id, library);
    assert_eq!(fact.title, recording.inherited_title);
    assert_eq!(fact.artist, recording.inherited_artist);
    assert_eq!(fact.duration_ms, 1000);
    assert_eq!(fact.sources.len(), 1);
    assert_eq!(fact.sources[0].id, source.id);
    assert_eq!(fact.sources[0].server_relay, ServerRelayObservation::Ready);
    let appearance = fact.sources[0].release.as_ref().unwrap();
    assert_eq!(Some(&appearance.id), recording.release_id.as_ref());
    assert_eq!(appearance.entry_id, source.id);
    let encoded = serde_json::to_string(&page).unwrap();
    for private in [
        dir.path().to_str().unwrap(),
        "private-server-host",
        "relative_path",
        "identity_key",
        "file_checksum",
        "scan_root",
        "device_id",
        "provider_id",
        "credential",
        "https://",
    ] {
        assert!(
            !encoded.contains(private),
            "export contains forbidden field or value: {private}"
        );
    }
    assert_eq!(std::fs::read(music.join("one.wav")).unwrap(), original);
    assert!(
        services
            .collection
            .events(&actor, 0)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    services.catalog_replication.close().await.unwrap();
}
