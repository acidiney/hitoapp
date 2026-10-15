#[path = "fixtures/legacy_library.rs"]
mod legacy;
mod support;
use hito_core::{CoreConfig, HostDeviceId, Search};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};

fn tag(path: &std::path::Path, title: &str) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.save_to_path(path, Default::default()).unwrap();
}

#[tokio::test]
async fn late_checksum_witness_is_scan_order_independent() {
    let mut totals = Vec::new();
    for name in ["0-witness.wav", "zz-witness.wav"] {
        let fixture = legacy::Legacy::seed().await;
        let original_bytes = std::fs::read(&fixture.original).unwrap();
        tag(&fixture.original, "Changed before upgrade");
        let (hito, actor) = fixture.open().await;
        support::scan(&hito, &actor, legacy::LIBRARY).await;
        assert_eq!(
            hito.catalog
                .recordings(&actor, Search::default())
                .await
                .unwrap()
                .total,
            2
        );
        let fresh = fixture.music.join("a-fresh.wav");
        std::fs::write(&fresh, &original_bytes).unwrap();
        tag(&fresh, "Another tag variant");
        std::fs::write(fixture.music.join(name), &original_bytes).unwrap();
        support::scan(&hito, &actor, legacy::LIBRARY).await;
        let recordings = hito
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        totals.push(recordings.total);
    }
    assert_eq!(
        totals,
        vec![3, 3],
        "historical ambiguity must be established before either filename is indexed"
    );
}

#[tokio::test]
async fn renaming_the_root_selects_a_surviving_metadata_owner() {
    let (dir, hito, actor, library, music) = support::library("server").await;
    let original = music.join("track.wav");
    support::write_wav(&original, 0);
    tag(&original, "Old title");
    support::scan(&hito, &actor, &library).await;
    let recording = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = hito
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    let new_root = dir.path().join("renamed-music");
    std::fs::rename(&music, &new_root).unwrap();
    tag(&new_root.join("track.wav"), "Corrected title");
    let moved = hito_core::open(CoreConfig::new(
        dir.path().join("hito.db"),
        new_root,
        HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    support::scan(&moved, &actor, &library).await;
    let current = moved
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert!(moved.catalog.audio_file(&actor, &source).await.is_ok());
    assert_eq!(current.items[0].title, "Corrected title");
}
