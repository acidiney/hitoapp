use std::{
    io::Write,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(super) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut capture = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("the log capture lock failed"))?;
        if bytes.len() > 65_536_usize.saturating_sub(capture.len()) {
            return Err(std::io::Error::other("the log capture exceeded its limit"));
        }
        capture.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter("off,hito_debrid_real_debrid=trace,hito_remote_audio=trace")
                .with_ansi(false)
                .without_time()
                .json()
                .with_writer(move || capture.clone())
                .finish(),
        )
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("the log capture must lock").clone())
            .expect("the owned observation logs must be UTF-8")
    }
}
