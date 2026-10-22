use hito_testkit::logs::{LogCapture, trace_filter};

#[derive(Clone, Default)]
pub(super) struct Capture(LogCapture);

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        self.0.dispatch(&trace_filter(&[
            "hito_debrid_real_debrid",
            "hito_remote_audio",
        ]))
    }

    pub fn text(&self) -> String {
        self.0.text()
    }
}
