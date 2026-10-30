#![cfg(feature = "device")]
use hito_core::test_support::playback as fixture;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tracing::instrument::WithSubscriber;

struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn an_owned_session_retains_workflow_logs_across_threads_without_private_input() {
    let f = fixture::LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = captured.clone();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Sink(writer.clone()))
            .finish(),
    );
    let session = hito_playback::Session::open(grant)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    session.close().with_subscriber(dispatch).await.unwrap();
    let logs = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("native_session_finished"),
        "the supervisor workflow log disappeared"
    );
    assert!(
        logs.contains("audio_player_closed"),
        "the blocking engine workflow log disappeared"
    );
    assert!(
        logs.contains("native_session="),
        "session events need a safe correlation ID"
    );
    assert!(!logs.contains(&f.token));
    assert!(!logs.contains(f.music.to_str().unwrap()));
    assert!(!logs.contains("indexing test password"));
}
