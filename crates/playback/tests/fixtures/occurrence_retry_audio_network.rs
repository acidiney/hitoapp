#[path = "occurrence_retry.rs"]
mod occurrence;

use axum::http::{HeaderMap, Method, StatusCode};
use hito_testkit::fake_plugin::{Fault, Gate, Reply};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};

const UNAVAILABLE: u8 = 0;
const AUDIO: u8 = 1;
const FORBIDDEN: u8 = 2;
const INVALID: u8 = 3;
const CREDENTIALS: u8 = 4;

pub struct Fixture {
    pub occurrence: occurrence::Fixture,
    mode: Arc<AtomicU8>,
    handler_calls: Arc<AtomicUsize>,
    unavailable_responses: Arc<AtomicUsize>,
}

impl Fixture {
    pub async fn start() -> Self {
        let occurrence = occurrence::Fixture::start_with_ready_on_attempt(Some(1)).await;
        let mode = Arc::new(AtomicU8::new(UNAVAILABLE));
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let unavailable_responses = Arc::new(AtomicUsize::new(0));
        let state = mode.clone();
        let calls = handler_calls.clone();
        let unavailable = unavailable_responses.clone();
        let invalid = occurrence.invalid.clone();
        let audio = std::fs::read(occurrence.directory.path().join("owned-provider.wav")).unwrap();
        occurrence.second.on("v1/audio", move |request| {
            let valid = request.method == Method::GET
                && request.query.as_deref() == Some("source_id=owned-ready-audio")
                && request.body.is_empty();
            if !valid {
                invalid.store(true, Ordering::SeqCst);
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            calls.fetch_add(1, Ordering::SeqCst);
            match state.load(Ordering::SeqCst) {
                AUDIO => Reply::Raw {
                    status: StatusCode::OK,
                    headers: HeaderMap::from_iter([(
                        "content-type".parse().unwrap(),
                        "audio/wav".parse().unwrap(),
                    )]),
                    body: audio.clone(),
                    chunked: false,
                },
                FORBIDDEN => Reply::Status(StatusCode::FORBIDDEN),
                CREDENTIALS => Reply::Status(StatusCode::UNAUTHORIZED),
                INVALID => Reply::Raw {
                    status: StatusCode::OK,
                    headers: HeaderMap::from_iter([(
                        "content-type".parse().unwrap(),
                        "application/octet-stream".parse().unwrap(),
                    )]),
                    body: b"owned invalid audio representation".to_vec(),
                    chunked: false,
                },
                _ => {
                    unavailable.fetch_add(1, Ordering::SeqCst);
                    Reply::Status(StatusCode::SERVICE_UNAVAILABLE)
                }
            }
        });
        Self {
            occurrence,
            mode,
            handler_calls,
            unavailable_responses,
        }
    }

    pub fn allow_audio(&self) {
        self.mode.store(AUDIO, Ordering::SeqCst);
    }

    pub fn refuse_audio(&self) {
        self.mode.store(FORBIDDEN, Ordering::SeqCst);
    }

    pub fn invalid_audio(&self) {
        self.mode.store(INVALID, Ordering::SeqCst);
    }

    pub fn credentials_audio(&self) {
        self.mode.store(CREDENTIALS, Ordering::SeqCst);
    }

    pub fn disconnect_before_headers(&self) {
        self.occurrence
            .second
            .fault("v1/audio", Fault::DropConnection);
    }

    pub fn clear_disconnect(&self) {
        self.occurrence.second.clear_fault("v1/audio");
    }

    pub fn hold_audio(&self) -> Arc<Gate> {
        self.occurrence.second.hold("v1/audio")
    }

    pub fn audio_requests(&self) -> usize {
        self.occurrence.second.requests_for("v1/audio").len()
    }

    pub fn handler_calls(&self) -> usize {
        self.handler_calls.load(Ordering::SeqCst)
    }

    pub fn unavailable_responses(&self) -> usize {
        self.unavailable_responses.load(Ordering::SeqCst)
    }

    pub fn private_values(&self) -> Vec<String> {
        self.occurrence.private_values()
    }

    pub async fn close(self) -> bool {
        self.occurrence.second.clear_fault("v1/audio");
        self.occurrence.close().await
    }

    pub async fn wait_audio_request(&self, count: usize) {
        loop {
            let total = self.occurrence.second.requests().len();
            if self.audio_requests() >= count {
                return;
            }
            self.occurrence.second.wait_for_requests(total + 1).await;
        }
    }
}
