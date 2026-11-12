#[derive(Clone)]
pub struct SourceStop(tokio_util::sync::CancellationToken);

impl SourceStop {
    pub(crate) fn new() -> Self {
        Self(tokio_util::sync::CancellationToken::new())
    }

    #[cfg(feature = "device")]
    pub(crate) fn child(&self) -> Self {
        Self(self.0.child_token())
    }

    pub fn request(&self) {
        if self.0.is_cancelled() {
            return;
        }
        self.0.cancel();
        tracing::debug!(
            event = "native_input_stop_requested",
            "Native input stop requested"
        );
    }

    pub(crate) fn requested(&self) -> bool {
        self.0.is_cancelled()
    }

    pub(crate) async fn cancelled(&self) {
        self.0.cancelled().await;
    }
}
