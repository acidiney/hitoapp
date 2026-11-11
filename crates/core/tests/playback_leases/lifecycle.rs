use super::fixture::Case;
use hito_core::{AudioRequest, OpenedAudio};

#[tokio::test]
async fn close_is_idempotent_joins_owners_and_invalidates_retained_bodies_before_storage_retirement()
 {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    f.services.playback.close().await.unwrap();
    f.services.playback.close().await.unwrap();
    assert_no_file_descriptor(&f);
    assert!(
        f.services
            .playback
            .begin_lease(&f.actor, f.request())
            .await
            .is_err()
    );
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
    f.close().await;
    assert!(
        body.chunk().await.is_err(),
        "retained response owns no Core/file resources"
    );
}

fn assert_no_file_descriptor(f: &Case) {
    #[cfg(target_os = "linux")]
    {
        let held = std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| std::fs::read_link(entry.path()).ok())
            .filter(|path| path == &f.path())
            .count();
        assert_eq!(
            held, 0,
            "retired owner must close the authorized descriptor even with a retained unpolled body"
        );
    }
    #[cfg(not(target_os = "linux"))]
    let _ = f;
}
