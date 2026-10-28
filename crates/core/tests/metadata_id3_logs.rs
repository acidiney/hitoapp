use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::test_support::support;

#[path = "fixtures/id3_tags.rs"]
mod id3;

use id3::{Frame, Storage, Version, tag, write_mp3_with_tag};

#[tokio::test(flavor = "current_thread")]
async fn ignored_and_decompressed_frames_are_counted_without_their_content() {
    let (dir, hito, actor, library, music) = support::library("id3-logs").await;
    let captured = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(captured.subscriber()).unwrap();
    let frames = vec![
        Frame::compressed_text("TIT2", "Private compressed title canary"),
        Frame {
            id: "TPE1",
            payload: Vec::new(),
            storage: Storage::CompressedBytes(vec![1, 2, 3, 4, 5, 6], 30),
            unsynchronised: false,
        },
    ];
    write_mp3_with_tag(
        dir.path(),
        &music.join("private-name-canary.mp3"),
        &tag(Version::V24, false, &frames),
    );
    support::scan(&hito, &actor, &library).await;
    let text = captured.text();
    assert!(text.contains("id3_compressed_frames"), "{text}");
    assert!(text.contains("\"decompressed\":1"), "{text}");
    assert!(text.contains("\"ignored_invalid\":1"), "{text}");
    assert!(!text.contains("canary"), "{text}");
}
