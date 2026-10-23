#[path = "fixtures/audiobooks_library.rs"]
mod library;
use hito_core::test_support::support;

use hito_core::Action;
use uuid::Uuid;

#[tokio::test]
async fn an_audiobook_retains_existing_chapter_identities_bookmarks_and_recording_progress() {
    let library = library::Library::prepare().await;
    let before = library.snapshot().await;
    let playlist = library.playlist.to_string();
    let entries = library.entries.map(|id| id.to_string());
    let recordings = library.recordings.map(|id| id.to_string());
    let chapters = &before["chapters"]["entries"];
    assert!(
        chapters[0]["entry_id"] == entries[0] && chapters[1]["entry_id"] == entries[1],
        "The ordered book must begin with the accepted chapter occurrences"
    );
    assert!(
        chapters[0]["recording_id"] == recordings[0]
            && chapters[1]["recording_id"] == recordings[1]
            && chapters[0]["position"] == 10
            && chapters[1]["position"] == 20,
        "Chapter order must follow the user's collection rather than catalog sorting"
    );
    assert!(
        before["metadata"][0]["title"]["personal"] == library::PERSONAL_TITLE
            && before["metadata"][0]["artist"]["personal"] == library::PERSONAL_ARTIST,
        "The source-backed chapters must retain their private title and narrator pins"
    );
    let unrelated = before["unrelated"]["entries"].as_array().unwrap();
    assert!(
        unrelated.len() == 2
            && unrelated[0]["recording_id"] == recordings[0]
            && unrelated[1]["recording_id"] == recordings[0]
            && unrelated[0]["entry_id"] != unrelated[1]["entry_id"],
        "The private preservation snapshot must contain distinct duplicate playlist occurrences"
    );
    library.assert_no_downloads().await;
    library.assert_originals_unchanged();
    let events_before = library
        .hito
        .collection
        .events(&library.actor, 0)
        .await
        .unwrap();
    let bookmark_id = Uuid::new_v4();
    let accepted = library::apply(
        &library.hito,
        &library.actor,
        vec![
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            Action::AudiobookBookmarkCreate {
                audiobook_id: library.playlist,
                bookmark_id,
                entry_id: library.entries[0],
                recording_id: library.recordings[0],
                position_ms: 250,
                label: "Private return point".into(),
            },
        ],
    )
    .await;
    let book = library
        .hito
        .collection
        .audiobook(&library.actor, &playlist)
        .await
        .unwrap();
    assert!(
        book.playlist.id == playlist
            && book.playlist.name == library::BOOK_TITLE
            && book.playlist.kind == "local",
        "An audiobook must retain its owned editable playlist identity"
    );
    assert!(
        serde_json::to_value(&book.chapters).unwrap() == *chapters,
        "The book must expose the exact existing chapter entries without copying or renaming them"
    );
    assert!(
        book.bookmarks.len() == 1,
        "The book must retain one explicitly created bookmark"
    );
    let bookmark = &book.bookmarks[0];
    assert!(
        bookmark.id == bookmark_id.to_string()
            && bookmark.audiobook_id == playlist
            && bookmark.entry_id == entries[0]
            && bookmark.recording_id == recordings[0]
            && bookmark.position_ms == 250
            && bookmark.label == "Private return point",
        "The private bookmark must retain its exact book, chapter occurrence, recording and position"
    );
    let retained_book = serde_json::to_value(&book).unwrap();
    assert!(
        library.snapshot().await == before,
        "Grouping and bookmarks must preserve catalog identities, metadata, sources, playlists, likes and progress"
    );
    let progress = library
        .hito
        .collection
        .saved_playback(&library.actor, &library.recordings)
        .await
        .unwrap();
    assert!(
        progress.len() == 2
            && progress[0].recording_id == library.recordings[0]
            && progress[0].position_ms == 100
            && progress[0].speed == 1.5
            && progress[1].recording_id == library.recordings[1]
            && progress[1].position_ms == 650
            && progress[1].speed == 1.25,
        "Bookmark positions must remain separate from each recording's existing saved position and speed"
    );
    let events_after = library
        .hito
        .collection
        .events(&library.actor, 0)
        .await
        .unwrap();
    assert!(
        events_after.operations.len() == events_before.operations.len() + 2,
        "Grouping and bookmarking must each publish exactly one accepted collection operation"
    );
    let encoded_events = serde_json::to_value(&events_after.operations).unwrap();
    let previous_events = serde_json::to_value(&events_before.operations).unwrap();
    assert!(
        encoded_events.as_array().unwrap()[..events_before.operations.len()]
            == *previous_events.as_array().unwrap(),
        "Existing accepted collection events must remain unchanged"
    );
    for (event, operation) in encoded_events.as_array().unwrap()[events_before.operations.len()..]
        .iter()
        .zip(accepted)
    {
        assert!(
            event["operation"]["id"] == operation.to_string(),
            "The new audiobook events must retain the acknowledged operation identities"
        );
    }
    library.assert_no_downloads().await;
    let reopened = library.reopen().await;
    let book = reopened
        .hito
        .collection
        .audiobook(&reopened.actor, &playlist)
        .await
        .unwrap();
    assert!(
        serde_json::to_value(book).unwrap() == retained_book,
        "The complete audiobook and private bookmark must survive reopening core"
    );
    assert!(
        reopened.snapshot().await == before,
        "Reopening must retain every existing musical and personal snapshot exactly"
    );
    reopened.assert_no_downloads().await;
    reopened.assert_originals_unchanged();
}
