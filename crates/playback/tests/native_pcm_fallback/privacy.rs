use super::fixture::Library;
use hito_core::test_support::media_tool;
use hito_playback::Session;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tracing::instrument::WithSubscriber;

#[derive(Clone)]
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
async fn fallback_and_preparation_keep_scoped_workflow_logs_without_private_diagnostics() {
    let f = Library::configured(|config, tools| {
        config.media_tools.ffmpeg = media_tool::script(tools, "ffmpeg", "case \"$*\" in *pcm_f32le*) printf 'private-decoder-diagnostic-url' >&2;; esac\nexec /usr/bin/ffmpeg \"$@\"");
    }).await;
    let logs = Sink(Arc::new(Mutex::new(Vec::new())));
    let captured = logs.clone();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || captured.clone())
            .finish(),
    );
    let session = Session::open(f.grant().await)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    session
        .prepare_next(f.grant().await)
        .with_subscriber(dispatch.clone())
        .await
        .unwrap();
    assert!(
        session
            .cancel_next()
            .with_subscriber(dispatch.clone())
            .await
            .unwrap()
    );
    session.close().with_subscriber(dispatch).await.unwrap();
    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    for event in [
        "native_fallback_ready",
        "native_fallback_closed",
        "native_preparation_started",
        "native_preparation_finished",
        "native_pcm_closed",
        "native_session_finished",
        "native_session=",
        "playback_grant=",
    ] {
        assert!(logs.contains(event), "scoped native workflow lost {event}");
    }
    for private in [
        &f.token,
        f.path.parent().unwrap().to_str().unwrap(),
        "owned.opus",
        "indexing test password",
        "private-decoder-diagnostic-url",
        "/dev/stdin",
        "pipe:1",
    ] {
        assert!(
            !logs.contains(private),
            "private input escaped into workflow logs"
        );
    }
}
