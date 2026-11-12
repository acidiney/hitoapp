use super::fixture::{Library, open_with_diagnostics, until};
use hito_audio::test_support::capture::{Capture, energy};
use hito_playback::Command;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn repeated_capture_and_native_opens_remain_usable_for_original_and_pcm_decoders() {
    let f = Library::start().await;
    let original = f.wave().await;
    let compatibility = f.grant().await;
    for _ in 0..10 {
        for grant in [&original, &compatibility] {
            let capture = Capture::open();
            let session = open_with_diagnostics(grant.clone()).await;
            session.command(Command::Resume).await.unwrap();
            until(&session, |state| {
                state.position > Duration::from_millis(100)
            })
            .await;
            capture.after(capture.mark(), 0.05);
            assert!(energy(&capture.recent(0.05)) > 0.0001);
            session.close().await.unwrap();
        }
    }
}
