use std::{
    io::Write,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    pub fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        let captured = self.clone();
        tracing_subscriber::fmt()
            .with_env_filter("off,hito_core=trace")
            .with_ansi(false)
            .without_time()
            .json()
            .with_writer(move || captured.clone())
            .finish()
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
