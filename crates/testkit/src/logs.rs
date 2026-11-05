use serde_json::Value;
use std::{
    io::{self, Write},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

static CALLSITE_INTEREST_ANCHORS: OnceLock<[tracing::Dispatch; 2]> = OnceLock::new();

pub fn anchor_callsite_interest() {
    CALLSITE_INTEREST_ANCHORS.get_or_init(|| {
        [
            tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
            tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()),
        ]
    });
}

pub fn anchored_dispatch(subscriber: impl tracing::Subscriber + Send + Sync) -> tracing::Dispatch {
    anchor_callsite_interest();
    tracing::Dispatch::new(subscriber)
}

pub const MAX_CAPTURE_BYTES: usize = 65_536;

#[derive(Clone)]
pub struct LogCapture {
    bytes: Arc<Mutex<Vec<u8>>>,
    overflowed: Arc<AtomicBool>,
    limit: usize,
}

impl Default for LogCapture {
    fn default() -> Self {
        Self::with_limit(MAX_CAPTURE_BYTES)
    }
}

pub fn trace_filter(targets: &[&str]) -> String {
    targets.iter().fold(String::from("off"), |filter, target| {
        format!("{filter},{target}=trace")
    })
}

impl LogCapture {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            bytes: Arc::default(),
            overflowed: Arc::default(),
            limit,
        }
    }

    pub fn unbounded() -> Self {
        Self::with_limit(usize::MAX)
    }

    pub fn dispatch(&self, filter: &str) -> tracing::Dispatch {
        anchor_callsite_interest();
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .without_time()
                .json()
                .with_writer(move || capture.clone())
                .finish(),
        )
    }

    pub fn dispatch_with_spans(&self, filter: &str) -> tracing::Dispatch {
        anchor_callsite_interest();
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .without_time()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(move || capture.clone())
                .finish(),
        )
    }

    pub fn text(&self) -> String {
        assert!(
            !self.overflowed.load(Ordering::SeqCst),
            "owned log capture exceeded its bound"
        );
        let bytes = self.bytes.lock().expect("log capture must lock");
        assert!(bytes.len() <= self.limit);
        String::from_utf8(bytes.clone()).expect("structured logs must be UTF-8")
    }

    pub fn events(&self) -> Vec<Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).expect("logs must be JSON events"))
            .collect()
    }

    pub fn assert_excludes(&self, forbidden: &[&str]) {
        assert_text_excludes(&self.text(), forbidden);
    }
}

pub fn assert_text_excludes(text: &str, forbidden: &[&str]) {
    for secret in forbidden {
        assert!(!secret.is_empty(), "forbidden log text must not be empty");
        let escaped = serde_json::to_string(secret).expect("forbidden text must encode");
        assert!(
            !text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]),
            "logs exposed private fixture data"
        );
    }
}

impl Write for LogCapture {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut bytes = self
            .bytes
            .lock()
            .map_err(|_| io::Error::other("log capture lock failed"))?;
        if input.len() > self.limit.saturating_sub(bytes.len()) {
            self.overflowed.store(true, Ordering::SeqCst);
            return Err(io::Error::other("owned log capture exceeded its bound"));
        }
        bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
