#[path = "fixtures/audiobooks_deleted.rs"]
mod deleted;
#[path = "fixtures/audiobooks_library.rs"]
mod library;
#[path = "fixtures/audiobooks_peer.rs"]
mod peer;
mod support;

use hito_core::Action;
use uuid::Uuid;

#[tokio::test]
async fn deleted_owned_books_acknowledge_delayed_actions_without_poisoning_independent_sync_work() {
    let library = library::Library::prepare().await;
    let peer = peer::Peer::prepare(&library).await;
    assert!(
        peer.entry != library.entries[0],
        "The second account must retain its independent chapter occurrence"
    );
    let mut initial = library.snapshot().await;
    let unrelated =
        Uuid::parse_str(initial["unrelated"]["playlist"]["id"].as_str().unwrap()).unwrap();
    initial.as_object_mut().unwrap().remove("chapters");
    let marker = Uuid::new_v4();
    library::apply(
        &library.hito,
        &library.actor,
        vec![
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            create_marker(&library, marker),
        ],
    )
    .await;
    let book = library
        .hito
        .collection
        .audiobook(&library.actor, &library.playlist.to_string())
        .await
        .unwrap();
    assert!(
        book.bookmarks.len() == 1 && book.bookmarks[0].id == marker.to_string(),
        "The deletion regression must begin with one persisted bookmark in a live owned audiobook"
    );
    assert!(
        deleted::musical(&library, unrelated).await == initial,
        "Grouping and initial bookmarking must preserve the complete private musical projection"
    );
    let peer_before = peer.snapshot(&library).await;

    deleted::accept(
        &library,
        vec![
            Action::PlaylistDelete {
                playlist_id: library.playlist,
            },
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            create_marker(&library, Uuid::new_v4()),
            Action::AudiobookBookmarkRemove {
                audiobook_id: library.playlist,
                bookmark_id: marker,
            },
            Action::PlaylistRename {
                playlist_id: unrelated,
                name: "Private retained selection after deletion".into(),
            },
        ],
    )
    .await;
    deleted::assert_deleted(&library).await;
    let mut expected = deleted::expected_after_deletion(
        &initial,
        library.playlist,
        unrelated,
        "Private retained selection after deletion",
    );
    assert!(
        deleted::musical(&library, unrelated).await == expected
            && peer.snapshot(&library).await == peer_before,
        "The mixed deletion batch must preserve all private metadata, sources, progress, likes and chapter identities outside its explicit deletion and rename"
    );

    deleted::accept(
        &library,
        vec![
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            create_marker(&library, marker),
            Action::AudiobookBookmarkRemove {
                audiobook_id: library.playlist,
                bookmark_id: Uuid::new_v4(),
            },
            Action::PlaylistCreate {
                playlist_id: library.playlist,
                name: "Private delayed book recreation".into(),
            },
            Action::PlaylistRename {
                playlist_id: unrelated,
                name: "Private independently committed selection".into(),
            },
        ],
    )
    .await;
    deleted::rename(
        &mut expected,
        unrelated,
        "Private independently committed selection",
    );
    deleted::assert_deleted(&library).await;
    let retained_events = deleted::events(&library).await;
    assert!(
        deleted::musical(&library, unrelated).await == expected
            && peer.snapshot(&library).await == peer_before,
        "Delayed operations after committed deletion must remain no-ops while independent work still commits"
    );

    for action in [
        Action::AudiobookCreate {
            playlist_id: library.playlist,
        },
        create_marker(&library, marker),
        Action::AudiobookBookmarkRemove {
            audiobook_id: library.playlist,
            bookmark_id: marker,
        },
    ] {
        deleted::deny(&library, &peer.actor, action).await;
    }
    assert!(
        deleted::musical(&library, unrelated).await == expected
            && deleted::events(&library).await == retained_events
            && peer.snapshot(&library).await == peer_before,
        "Foreign deleted-book denials must leave both accounts and every accepted event unchanged"
    );
    library.assert_no_downloads().await;
    peer.assert_no_downloads(&library).await;
    library.assert_originals_unchanged();
    let reopened = library.reopen().await;
    deleted::assert_deleted(&reopened).await;
    assert!(
        deleted::musical(&reopened, unrelated).await == expected
            && deleted::events(&reopened).await == retained_events
            && peer.snapshot(&reopened).await == peer_before,
        "Committed deletion, independent edits and both private accounts must survive reopening without any book or marker resurrection"
    );
    reopened.assert_no_downloads().await;
    peer.assert_no_downloads(&reopened).await;
    reopened.assert_originals_unchanged();
}

fn create_marker(library: &library::Library, bookmark_id: Uuid) -> Action {
    Action::AudiobookBookmarkCreate {
        audiobook_id: library.playlist,
        bookmark_id,
        entry_id: library.entries[0],
        recording_id: library.recordings[0],
        position_ms: 250,
        label: "Private deleted-book return point".into(),
    }
}
