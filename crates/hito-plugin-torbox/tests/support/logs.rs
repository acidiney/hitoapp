use super::{ARTIST, COOKIE, QUERY, SERVICE_TOKEN, TITLE, WRONG_TOKEN, fixtures};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
    overflowed: Arc<AtomicBool>,
}

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        let capture = self.clone();
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(
                    "off,hito_plugin_torbox=trace,hito_debrid_torbox=trace,hito_remote_audio=trace",
                )
                .without_time()
                .with_ansi(false)
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(move || capture.clone())
                .finish(),
        )
    }

    pub fn assert_workflow(&self, statuses: &[u16], handle: &str) {
        assert!(!self.overflowed.load(Ordering::SeqCst));
        let bytes = self.bytes.lock().unwrap();
        assert!(!bytes.is_empty());
        let text = std::str::from_utf8(&bytes).unwrap();
        for private in [
            fixtures::TOKEN,
            SERVICE_TOKEN,
            WRONG_TOKEN,
            COOKIE,
            QUERY,
            TITLE,
            ARTIST,
            "0123456789abcdef0123456789abcdef01234567",
            "album/owned.wav",
            fixtures::MEDIA_TARGET,
            "owned-torbox-key%2F%2B%3F%3D%26-kept-private",
            "owned-private-observation-detail",
            "owned-private-torrent-title",
            "owned-private-media-cookie",
            "owned-private-provider-name.wav",
            "http://",
            "https://",
            "Bearer ",
            "token=",
            handle,
        ] {
            assert!(!private.is_empty());
            let escaped = serde_json::to_string(private).unwrap();
            assert!(
                !text.contains(private) && !text.contains(&escaped[1..escaped.len() - 1]),
                "private provider or caller data reached captured workflow logs"
            );
        }
        let events: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut actual_statuses = Vec::new();
        let mut request_ids = BTreeSet::new();
        let mut observations = Vec::new();
        let mut preparations = Vec::new();
        let mut opening = None;
        let mut body = None;
        for event in &events {
            assert_eq!(event["level"], "DEBUG");
            let fields = &event["fields"];
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"].as_array().unwrap();
            assert!(!spans.is_empty());
            assert_eq!(&event["span"], spans.last().unwrap());
            for span in spans {
                assert!(span["operation"].as_str().is_some());
                assert_eq!(
                    Uuid::parse_str(span["request_id"].as_str().unwrap())
                        .unwrap()
                        .get_version_num(),
                    4
                );
            }
            let id = event["span"]["request_id"].as_str().unwrap();
            match fields["event"].as_str().unwrap() {
                "torbox_plugin_request_finished" => {
                    assert!(request_ids.insert(id));
                    actual_statuses.push(fields["status"].as_u64().unwrap() as u16);
                    assert_eq!(
                        fields["outcome"],
                        match actual_statuses.last().unwrap() {
                            200..=299 => "success",
                            401 => "unauthorized",
                            _ => "rejected",
                        }
                    );
                }
                "torbox_observation_finished" => {
                    observations.push(fields["outcome"].as_str().unwrap())
                }
                "torbox_preparation_finished" => {
                    preparations.push(fields["outcome"].as_str().unwrap())
                }
                "torbox_audio_opening_finished" => {
                    assert!(opening.replace(id).is_none());
                    assert_eq!(fields["outcome"], "complete");
                    assert_eq!(fields["count"], 1);
                }
                "remote_audio_body_finished" => {
                    assert!(body.replace(id).is_none());
                    assert_eq!(fields["outcome"], "complete");
                    assert_eq!(fields["bytes"], fixtures::WAV_BYTES);
                }
                _ => panic!("an unqualified event reached workflow logs"),
            }
        }
        assert_eq!(actual_statuses, statuses);
        assert_eq!(
            observations,
            [
                "incomplete",
                "provider_completed",
                "provider_completed",
                "provider_completed",
                "provider_completed"
            ]
        );
        assert_eq!(preparations, ["incomplete", "candidate"]);
        assert!(opening.is_some() && opening == body);
    }
}

impl Write for Capture {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut bytes = self.bytes.lock().unwrap();
        if input.len() > 65_536_usize.saturating_sub(bytes.len()) {
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
