use super::fixture::{Host, Library};
use hito_core::{
    Action, CatalogSnapshot, CatalogSnapshotMode, Operation, ReplicaSession,
    ServerRelayObservation, SyncBatch,
    test_support::support::{scan, write_wav},
};
use serde_json::Value;
use uuid::Uuid;

pub async fn cached_pages(
    local: &Library,
    session: &ReplicaSession,
    snapshot: &CatalogSnapshot,
) -> Vec<Value> {
    let mut pages = Vec::new();
    for ordinal in 0..snapshot.page_count {
        let page = local
            .services
            .replicas
            .catalog_page(&local.actor, session, snapshot.id, ordinal)
            .await
            .unwrap();
        assert_eq!(page.snapshot_id, snapshot.id);
        assert_eq!(page.ordinal, ordinal);
        assert_eq!(
            page.next_ordinal,
            (ordinal + 1 < snapshot.page_count).then_some(ordinal + 1)
        );
        pages.push(serde_json::to_value(page).unwrap());
    }
    pages
}

pub fn tag_wav(path: &std::path::Path, title: &str) {
    use lofty::{
        prelude::{Accessor, TagExt},
        tag::{Tag, TagType},
    };
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Owned replica performer".into());
    tag.set_album("Owned replica release".into());
    tag.set_track(3);
    tag.set_disk(2);
    tag.save_to_path(path, Default::default()).unwrap();
}

pub async fn create_playlist(library: &Library, name: &str) -> Uuid {
    let playlist_id = Uuid::new_v4();
    library
        .services
        .collection
        .apply(
            &library.actor,
            SyncBatch {
                device_id: "owned-acceptance-client".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistCreate {
                        playlist_id,
                        name: name.into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    playlist_id
}

pub async fn multipage_snapshot(remote: &Host) -> (String, CatalogSnapshot, Vec<Value>) {
    let server_library = remote
        .library
        .services
        .catalog
        .libraries(&remote.library.actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    let music = remote.library.directory.path().join("music");
    for index in 0..47 {
        let path = music.join(format!("{index:03}.wav"));
        write_wav(&path, index);
        tag_wav(
            &path,
            &format!("Owned {index:03} {}", "\"\\\n\t".repeat(700)),
        );
    }
    scan(
        &remote.library.services,
        &remote.library.actor,
        &server_library,
    )
    .await;
    create_playlist(&remote.library, "Accepted before catalog bootstrap").await;
    let expected = remote
        .library
        .services
        .catalog_replication
        .begin(&remote.library.actor, &server_library)
        .await
        .unwrap();
    assert_eq!(expected.recording_count, 47);
    assert!(expected.page_count > 1);
    assert_eq!(expected.mode, CatalogSnapshotMode::SnapshotOnly);
    assert_eq!(expected.collection_watermark, 1);
    let mut expected_pages = Vec::new();
    let mut largest_page = 0;
    for ordinal in 0..expected.page_count {
        let page = remote
            .library
            .services
            .catalog_replication
            .page(&remote.library.actor, expected.id, ordinal)
            .await
            .unwrap();
        largest_page = largest_page.max(serde_json::to_vec(&page).unwrap().len());
        for recording in &page.recordings {
            assert_eq!(recording.library_id, server_library);
            assert_eq!(recording.sources.len(), 1);
            let source = &recording.sources[0];
            assert_eq!(source.server_relay, ServerRelayObservation::Ready);
            let release = source.release.as_ref().unwrap();
            assert_eq!(release.entry_id, source.id);
            assert_eq!(release.title, "Owned replica release");
            assert_eq!(release.disc_number, Some(2));
            assert_eq!(release.track_number, Some(3));
        }
        expected_pages.push(serde_json::to_value(page).unwrap());
    }
    assert!(largest_page > 240_000);

    (server_library, expected, expected_pages)
}
