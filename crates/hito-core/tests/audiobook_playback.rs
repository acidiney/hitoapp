#[path = "fixtures/audiobook_continue_library.rs"]
mod fixture;
#[path = "fixtures/content_service.rs"]
mod provider;
mod support;

use hito_core::{Action, AudiobookChapterAnchor, Error};
use serde_json::json;

#[tokio::test]
async fn fresh_audiobook_playback_keeps_the_exact_target_and_authorized_unready_chapters_after_an_unrelated_grant_is_revoked()
 {
    let f = fixture::Library::start().await;
    let rendered = f
        .services
        .collection
        .audiobook(&f.actor, &f.book.to_string())
        .await
        .unwrap();
    assert_eq!(rendered.chapters.len(), 4);
    assert_eq!(
        rendered.continuation.unwrap().chapter.entry_id,
        f.entries[0]
    );
    assert_eq!(
        rendered.chapters[1].recording_id.as_deref(),
        Some(f.recordings[1].to_string().as_str())
    );
    assert!(!rendered.chapters[1].available && !rendered.chapters[2].available);
    assert!(matches!(
        f.services
            .catalog
            .recordings_by_ids(&f.actor, &[f.recordings[1]])
            .await,
        Err(Error::NotFound)
    ));
    let unready = f
        .services
        .catalog
        .recordings_by_ids(&f.actor, &[f.recordings[2]])
        .await
        .unwrap();
    assert_eq!(unready.len(), 1);
    assert!(!unready[0].available && unready[0].liked);
    let expected = AudiobookChapterAnchor {
        audiobook_id: f.book,
        entry_id: f.entries[3],
        recording_id: f.recordings[0],
    };
    support::synchronize(
        &f.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: f.recordings[0],
                position_ms: 750,
                speed: 0.75,
            },
            Action::AudiobookChapterSet { chapter: expected },
        ],
    )
    .await;
    let before = f.snapshot().await;
    assert_eq!(before["source_choice"]["mode"], "manual");
    let prepared = f
        .services
        .collection
        .audiobook_playback(&f.actor, &f.book.to_string())
        .await
        .expect("An unrelated denied chapter must not block an authorized ready continuation");
    assert_eq!(prepared.audiobook_id, f.book);
    assert_eq!(prepared.chapter, expected);
    assert_eq!(prepared.progress.recording_id, f.recordings[0]);
    assert_eq!(prepared.progress.position_ms, 750);
    assert_eq!(prepared.progress.speed, 0.75);
    assert_eq!(prepared.chapters.len(), 3);
    assert_eq!(
        prepared
            .chapters
            .iter()
            .map(|chapter| chapter.entry_id.clone())
            .collect::<Vec<_>>(),
        [f.entries[0], f.entries[2], f.entries[3]]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        prepared
            .chapters
            .iter()
            .map(|chapter| chapter.recording_id.clone())
            .collect::<Vec<_>>(),
        [f.recordings[0], f.recordings[2], f.recordings[0]]
            .iter()
            .map(|id| Some(id.to_string()))
            .collect::<Vec<_>>()
    );
    assert!(
        prepared.chapters[0].available
            && !prepared.chapters[1].available
            && prepared.chapters[2].available
    );
    assert!(prepared.chapters[1].liked);
    assert_eq!(
        prepared.chapters[2].title,
        "Private corrected chapter title"
    );
    assert_eq!(prepared.chapters[2].artist, "Private corrected narrator");
    assert_eq!(
        f.snapshot().await,
        before,
        "Preparing playback must preserve private fields, likes, source overrides, progress, book structure and events"
    );
    assert!(matches!(
        f.services
            .collection
            .audiobook(&f.admin, &f.book.to_string())
            .await,
        Err(Error::NotFound)
    ));
    assert!(
        f.services
            .collection
            .activity(&f.admin)
            .await
            .unwrap()
            .history
            .is_empty()
    );
    for recording in f.recordings {
        assert!(matches!(
            f.services
                .downloads
                .status(&f.actor, &recording.to_string())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(std::fs::read(&f.original_path).unwrap(), f.original);
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert_eq!(
        serde_json::to_value(prepared.progress).unwrap(),
        json!({
            "recording_id": f.recordings[0], "position_ms": 750, "speed": 0.75,
        })
    );
    assert!(f.directory.path().join("hito.db").is_file());
}
