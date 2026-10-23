#[derive(Clone)]
pub struct SourceStop(tokio::sync::watch::Sender<bool>);

impl SourceStop {
    pub(crate) fn new() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }

    pub fn request(&self) {
        if self.0.send_replace(true) {
            return;
        }
        tracing::debug!(
            event = "native_input_stop_requested",
            "Native input stop requested"
        );
    }

    pub(crate) fn requested(&self) -> bool {
        *self.0.borrow()
    }

    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|value| *value).await;
    }
}
