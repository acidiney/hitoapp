#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, CollectionPublication, ImportProviderPlaylist, InstallPlugin, OfferPluginService,
    Operation, SyncBatch, SyncEvent,
};
use serde_json::json;
use uuid::Uuid;

const MIB: usize = 1024 * 1024;
const RECORDINGS: usize = 1_600;

#[tokio::test]
async fn real_import_publications_page_by_encoded_bytes_with_neighboring_client_events() {
    let server = fixture::ProviderServer::start().await;
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-byte-fixture", "name": "Playlist byte fixture",
                "protocol_version": 1, "capabilities": ["playlists"]
            }),
        )
        .await;
    let detail = json!({
        "id": "playlist/large", "name": "Immutable selection", "snapshot": "immutable-one",
        "total_count": RECORDINGS, "attribution": {"label": "Playlist byte fixture"}
    });
    let title = format!("{}{}", "é".repeat(700), "\"".repeat(600));
    let artist = format!("{}{}", "界".repeat(400), "\\".repeat(800));
    assert!(title.len() <= 2048 && artist.len() <= 2048);
    let items = (0..RECORDINGS)
        .map(|index| {
            json!({
                "occurrence_id": format!("occurrence-{index}"),
                "item": {"kind": "recording", "recording": {
                    "id": format!("recording/{index}"), "title": title, "artist": artist
                }}
            })
        })
        .collect::<Vec<_>>();
    let mut source_bytes = serde_json::to_vec(&detail).unwrap().len() * 2;
    let mut largest_source_page = 0;
    let mut pages = Vec::new();
    for (index, chunk) in items.chunks(100).enumerate() {
        let cursor = (index != 0).then(|| format!("page-{index}"));
        let next = ((index + 1) * 100 < RECORDINGS).then(|| format!("page-{}", index + 1));
        let page = json!({
            "playlist_id": "playlist/large", "snapshot": "immutable-one",
            "items": chunk, "next_cursor": next
        });
        assert!(chunk.len() <= 100);
        let encoded = serde_json::to_vec(&page).unwrap().len();
        assert!(encoded <= MIB);
        largest_source_page = largest_source_page.max(encoded);
        source_bytes += encoded;
        pages.push((cursor, page));
    }
    assert_eq!(pages.len(), 16);
    assert!(source_bytes <= 16 * MIB);
    server.reply("v1/playlists/detail", detail).await;
    server.playlist_pages(pages).await;
    let (_dir, hito, actor, _shared, _music) = support::library("playlist-event-bytes").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let neighbor = Uuid::new_v4();
    let before = Uuid::new_v4();
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "byte-boundary-client".into(),
                operations: vec![Operation {
                    id: before,
                    action: Action::PlaylistCreate {
                        playlist_id: neighbor,
                        name: "Neighbor".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let request = || ImportProviderPlaylist {
        request_id: Uuid::new_v4(),
        installation_revision: installed.revision,
        playlist_reference: "https://provider.invalid/immutable-selection".into(),
    };
    let first = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    let between = Uuid::new_v4();
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "byte-boundary-client".into(),
                operations: vec![Operation {
                    id: between,
                    action: Action::PlaylistRename {
                        playlist_id: first.playlist_id,
                        name: "Edited first copy".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();
    let second = hito
        .collection
        .import_provider_playlist(&actor, &installed.id, request())
        .await
        .unwrap();
    let after = Uuid::new_v4();
    hito.collection
        .apply(
            &actor,
            SyncBatch {
                device_id: "byte-boundary-client".into(),
                operations: vec![Operation {
                    id: after,
                    action: Action::PlaylistRename {
                        playlist_id: second.playlist_id,
                        name: "Edited second copy".into(),
                    },
                }],
            },
        )
        .await
        .unwrap();

    let first_page = hito.collection.events(&actor, 0).await.unwrap();
    let first_page_bytes = serde_json::to_vec(&first_page).unwrap().len();
    assert!(first_page_bytes <= 16 * MIB);
    assert_eq!(first_page.operations.len(), 3);
    assert!(first_page.has_more);
    assert_eq!(
        first_page.cursor,
        first_page.operations.last().unwrap().sequence()
    );
    let second_page = hito
        .collection
        .events(&actor, first_page.cursor)
        .await
        .unwrap();
    let second_page_bytes = serde_json::to_vec(&second_page).unwrap().len();
    assert!(second_page_bytes <= 16 * MIB);
    assert_eq!(second_page.operations.len(), 2);
    assert!(!second_page.has_more);
    assert_eq!(
        second_page.cursor,
        second_page.operations.last().unwrap().sequence()
    );
    let events = first_page
        .operations
        .iter()
        .chain(second_page.operations.iter())
        .collect::<Vec<_>>();
    let mut publication_bytes = Vec::new();
    let keys = events
        .iter()
        .map(|event| match event {
            SyncEvent::Operation {
                device_id,
                operation,
                ..
            } => {
                assert_eq!(device_id, "byte-boundary-client");
                format!("operation/{}", operation.id)
            }
            SyncEvent::Publication {
                publication: CollectionPublication::PlaylistImported { snapshot, .. },
                ..
            } => {
                let bytes = serde_json::to_vec(event).unwrap().len();
                assert!((8 * MIB..=10 * MIB).contains(&bytes));
                publication_bytes.push(bytes);
                assert_eq!(snapshot.name, "Immutable selection");
                assert_eq!(snapshot.entries.len(), RECORDINGS);
                assert_eq!(snapshot.recordings.len(), RECORDINGS);
                assert_eq!(
                    snapshot
                        .entries
                        .iter()
                        .map(|entry| entry.entry_id)
                        .collect::<std::collections::HashSet<_>>()
                        .len(),
                    RECORDINGS
                );
                format!("playlist/{}", snapshot.playlist_id)
            }
            SyncEvent::Publication {
                publication:
                    CollectionPublication::PlaylistFollowed { .. }
                    | CollectionPublication::PlaylistFollowRefreshed { .. },
                ..
            } => panic!("the import byte-boundary fixture must not publish followed playlists"),
            SyncEvent::Publication {
                publication: CollectionPublication::PlaylistMetadataRefreshed { .. },
                ..
            } => panic!("the import byte-boundary fixture must not publish metadata refreshes"),
        })
        .collect::<Vec<_>>();
    println!(
        "source_bytes={source_bytes} largest_source_page={largest_source_page} publication_bytes={publication_bytes:?} sync_page_bytes=[{first_page_bytes}, {second_page_bytes}]"
    );
    assert_eq!(
        keys,
        vec![
            format!("operation/{before}"),
            format!("playlist/{}", first.playlist_id),
            format!("operation/{between}"),
            format!("playlist/{}", second.playlist_id),
            format!("operation/{after}"),
        ]
    );
    assert_eq!(
        keys.iter().collect::<std::collections::HashSet<_>>().len(),
        5
    );
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].sequence() < pair[1].sequence())
    );
    let complete = hito
        .collection
        .events(&actor, second_page.cursor)
        .await
        .unwrap();
    assert!(complete.operations.is_empty());
    assert_eq!(complete.cursor, second_page.cursor);
    assert!(!complete.has_more);
    let names = hito.collection.playlists(&actor).await.unwrap();
    let first_id = first.playlist_id.to_string();
    let second_id = second.playlist_id.to_string();
    assert_eq!(
        names
            .iter()
            .find(|playlist| playlist.id == first_id)
            .unwrap()
            .name,
        "Edited first copy"
    );
    assert_eq!(
        names
            .iter()
            .find(|playlist| playlist.id == second_id)
            .unwrap()
            .name,
        "Edited second copy"
    );
    assert_eq!(server.requests("v1/playlists/detail").await.len(), 4);
    assert_eq!(server.requests("v1/playlists/items").await.len(), 32);
}
