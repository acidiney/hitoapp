#[path = "../fixtures/legacy_library.rs"]
mod legacy;
use hito_core::test_support::support;

use hito_core::Search;
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};

#[tokio::test]
async fn unchanged_legacy_audio_anchors_a_retagged_copy_before_scan_order_can_fork_it() {
    let fixture = legacy::Legacy::seed().await;
    let copy = fixture.music.join("a-retagged-copy.wav");
    std::fs::copy(&fixture.original, &copy).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("A corrected title".into());
    tag.save_to_path(&copy, Default::default()).unwrap();
    let (hito, actor) = fixture.open().await;
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].id, legacy::RECORDING);
    assert!(recordings.items[0].available && recordings.items[0].liked);
    let sources = hito
        .catalog
        .sources(&actor, legacy::RECORDING)
        .await
        .unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| source.ready));
    assert!(sources.iter().any(|source| source.id == legacy::SOURCE));
    let entries = hito
        .collection
        .playlist(&actor, legacy::PLAYLIST)
        .await
        .unwrap()
        .entries;
    assert_eq!(entries[0].entry_id, legacy::ENTRY);
    assert_eq!(entries[0].recording_id, Some(legacy::RECORDING.to_string()));
    let history = hito.collection.activity(&actor).await.unwrap().history;
    assert_eq!(history[0].id, legacy::RECORDING);
    assert_eq!(history[0].position_ms, 500);
    assert_eq!(history[0].speed, 1.25);
}

#[tokio::test]
async fn a_changed_legacy_file_without_an_audio_witness_stays_separate() {
    let fixture = legacy::Legacy::seed().await;
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Changed before upgrade".into());
    tag.save_to_path(&fixture.original, Default::default())
        .unwrap();
    let (hito, actor) = fixture.open().await;
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    let old = recordings
        .items
        .iter()
        .find(|r| r.id == legacy::RECORDING)
        .unwrap();
    assert!(old.liked && !old.available);
    let current = recordings
        .items
        .iter()
        .find(|r| r.id != legacy::RECORDING)
        .unwrap();
    assert!(current.available && !current.liked);
    let playlist = hito
        .collection
        .playlist(&actor, legacy::PLAYLIST)
        .await
        .unwrap();
    assert_eq!(playlist.entries[0].recording_id, Some(old.id.clone()));
    assert!(!playlist.entries[0].available);
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    assert_eq!(
        hito.catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .total,
        2
    );
}

#[cfg(unix)]
use hito_core::test_support::media_tool;

#[cfg(unix)]
#[tokio::test]
async fn audio_inspection_retries_unchanged_legacy_audio_after_decoder_failure() {
    use hito_core::{CoreConfig, HostDeviceId};

    let fixture = legacy::Legacy::seed().await;
    let failure = fixture.directory.path().join("fail-decoder");
    let calls = fixture.directory.path().join("decoder-calls");
    std::fs::write(&failure, "").unwrap();
    let decoder = media_tool::script(
        fixture.directory.path(),
        "ffmpeg",
        &format!(
            "printf 'decode\\n' >> {}\nif [ -e {} ]; then exit 1; fi\nexec /usr/bin/ffmpeg \"$@\"",
            media_tool::quote(&calls),
            media_tool::quote(&failure),
        ),
    );
    let mut config = CoreConfig::new(
        fixture.directory.path().join("hito.db"),
        fixture.music.clone(),
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffmpeg = decoder;
    let hito = hito_core::open(config).await.unwrap();
    let actor = hito
        .identity
        .authenticate("legacy-fixture-token")
        .await
        .unwrap();

    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let unavailable = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(unavailable.total, 1);
    assert_eq!(unavailable.items[0].id, legacy::RECORDING);
    assert!(unavailable.items[0].liked && !unavailable.items[0].available);
    let source = hito
        .catalog
        .sources(&actor, legacy::RECORDING)
        .await
        .unwrap();
    assert_eq!(source.len(), 1);
    assert_eq!(source[0].id, legacy::SOURCE);
    assert!(!source[0].ready);
    let failed_calls = std::fs::read_to_string(&calls).unwrap().lines().count();

    std::fs::remove_file(failure).unwrap();
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    assert!(std::fs::read_to_string(&calls).unwrap().lines().count() > failed_calls);
    let recovered = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recovered.total, 1);
    assert_eq!(recovered.items[0].id, legacy::RECORDING);
    assert!(recovered.items[0].liked && recovered.items[0].available);

    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Retagged after recovery".into());
    tag.save_to_path(&fixture.original, Default::default())
        .unwrap();
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let retagged = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(retagged.total, 1);
    assert_eq!(retagged.items[0].id, legacy::RECORDING);
    assert!(retagged.items[0].liked && retagged.items[0].available);
    assert_eq!(retagged.items[0].title, "Retagged after recovery");
    let sources = hito
        .catalog
        .sources(&actor, legacy::RECORDING)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, legacy::SOURCE);
    assert!(sources[0].ready);
    let playlist = hito
        .collection
        .playlist(&actor, legacy::PLAYLIST)
        .await
        .unwrap();
    assert_eq!(playlist.entries[0].entry_id, legacy::ENTRY);
    assert_eq!(
        playlist.entries[0].recording_id,
        Some(legacy::RECORDING.to_string())
    );
    assert!(playlist.entries[0].available);
    let history = hito.collection.activity(&actor).await.unwrap().history;
    assert_eq!(history[0].id, legacy::RECORDING);
    assert_eq!(history[0].position_ms, 500);
}

#[tokio::test]
async fn a_new_checksum_witness_preserves_legacy_identity_in_both_scan_orders() {
    for name in ["a-witness.wav", "zz-witness.wav"] {
        let fixture = legacy::Legacy::seed().await;
        std::fs::copy(&fixture.original, fixture.music.join(name)).unwrap();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Corrected before upgrade".into());
        tag.save_to_path(&fixture.original, Default::default())
            .unwrap();
        let (hito, actor) = fixture.open().await;
        support::scan(&hito, &actor, legacy::LIBRARY).await;
        let recordings = hito
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(recordings.total, 1, "witness {name}");
        assert_eq!(recordings.items[0].id, legacy::RECORDING);
        assert!(recordings.items[0].liked && recordings.items[0].available);
        let sources = hito
            .catalog
            .sources(&actor, legacy::RECORDING)
            .await
            .unwrap();
        assert_eq!(sources.len(), 2);
        assert!(sources.iter().all(|source| source.ready));
        assert!(sources.iter().any(|source| source.id == legacy::SOURCE));
        let playlist = hito
            .collection
            .playlist(&actor, legacy::PLAYLIST)
            .await
            .unwrap();
        assert_eq!(playlist.entries[0].entry_id, legacy::ENTRY);
        assert_eq!(
            playlist.entries[0].recording_id,
            Some(legacy::RECORDING.to_string())
        );
    }
}

#[tokio::test]
async fn a_checksum_witness_in_a_new_root_anchors_the_same_host_library() {
    use hito_core::{CoreConfig, HostDeviceId};
    let fixture = legacy::Legacy::seed().await;
    let root = fixture.directory.path().join("new-root");
    std::fs::create_dir(&root).unwrap();
    let retagged = root.join("a-retagged.wav");
    std::fs::copy(&fixture.original, &retagged).unwrap();
    std::fs::copy(&fixture.original, root.join("zz-witness.wav")).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Corrected in new root".into());
    tag.save_to_path(&retagged, Default::default()).unwrap();
    let hito = hito_core::open(CoreConfig::new(
        fixture.directory.path().join("hito.db"),
        root,
        HostDeviceId::new("server").unwrap(),
    ))
    .await
    .unwrap();
    let actor = hito
        .identity
        .authenticate("legacy-fixture-token")
        .await
        .unwrap();
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].id, legacy::RECORDING);
    assert!(recordings.items[0].liked && recordings.items[0].available);
}

#[cfg(unix)]
#[tokio::test]
async fn unchanged_legacy_locators_reuse_the_prepass_inspection() {
    use hito_core::{CoreConfig, HostDeviceId};
    let fixture = legacy::Legacy::seed().await;
    let calls = fixture.directory.path().join("decode-calls");
    let tool = media_tool::script(
        fixture.directory.path(),
        "ffmpeg",
        &format!(
            "if [ \"$1\" != '-version' ]; then printf 'decode\\n' >> {}; fi\nexec /usr/bin/ffmpeg \"$@\"",
            media_tool::quote(&calls),
        ),
    );
    let mut config = CoreConfig::new(
        fixture.directory.path().join("hito.db"),
        fixture.music.clone(),
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffmpeg = tool;
    let hito = hito_core::open(config).await.unwrap();
    let actor = hito
        .identity
        .authenticate("legacy-fixture-token")
        .await
        .unwrap();
    support::scan(&hito, &actor, legacy::LIBRARY).await;
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].id, legacy::RECORDING);
}
