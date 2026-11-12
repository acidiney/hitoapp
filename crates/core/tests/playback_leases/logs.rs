use super::fixture::{Case, content};
use hito_core::AudioRequest;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

struct Writer(Arc<Mutex<Vec<u8>>>);
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn lease_workflows_log_safe_outcomes_correlation_and_timings_without_private_material() {
    let f = Case::new().await;
    let token = f.token.clone();
    let private_path = f.path().to_string_lossy().into_owned();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let output = bytes.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || Writer(output.clone()))
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    f.services
        .playback
        .lease_status(&f.actor, lease.id)
        .await
        .unwrap();
    assert_eq!(
        content(
            f.services
                .playback
                .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
                .await
                .unwrap()
        )
        .await,
        f.original
    );
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    f.close().await;
    drop(guard);
    let log = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(log.contains("playback_lease_created"));
    assert!(log.contains("playback_lease_closed"));
    assert!(log.contains("playback_lease="));
    assert!(log.contains("elapsed_ms="));
    assert!(log.contains("outcome="));
    assert!(!log.contains(&token));
    assert!(!log.contains(&private_path));
    assert!(!log.contains(&lease.id.to_string()));
}
