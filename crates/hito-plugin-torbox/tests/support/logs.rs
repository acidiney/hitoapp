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
        let mut persistence = Vec::new();
        let mut lifecycle = Vec::new();
        let mut completed_commands = 0;
        let mut rejected_commands = 0;
        let mut transitions = Vec::new();
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
            let state_event = |target: &str, name: &str| {
                assert_eq!(event["target"], target);
                assert_eq!(event["span"]["name"], name);
                let root = event.as_object().unwrap();
                assert_eq!(root.len(), 5);
                assert!(root.keys().all(|key| matches!(
                    key.as_str(),
                    "level" | "fields" | "target" | "span" | "spans"
                )));
                let fields = fields.as_object().unwrap();
                assert_eq!(fields.len(), 4);
                assert!(fields.keys().all(|key| matches!(
                    key.as_str(),
                    "event" | "outcome" | "count" | "elapsed_ms"
                )));
                for span in spans {
                    let span = span.as_object().unwrap();
                    assert_eq!(span.len(), 3);
                    assert!(
                        span.keys()
                            .all(|key| matches!(key.as_str(), "name" | "request_id" | "operation"))
                    );
                }
            };
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
                name @ ("torbox_candidate_authority_finished"
                | "torbox_candidate_sealing_finished"
                | "torbox_candidate_restoration_finished") => {
                    state_event(
                        "hito_debrid_torbox::workflow",
                        "torbox_candidate_persistence",
                    );
                    let (operation, outcome) = match name {
                        "torbox_candidate_authority_finished" => {
                            ("candidate_authority", "complete")
                        }
                        "torbox_candidate_sealing_finished" => ("candidate_sealing", "sealed"),
                        "torbox_candidate_restoration_finished" => {
                            ("candidate_restoration", "restored")
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(event["span"]["operation"], operation);
                    assert_eq!(fields["outcome"], outcome);
                    assert_eq!(fields["count"], 1);
                    persistence.push(operation);
                }
                name @ ("torbox_journal_open_finished" | "torbox_journal_close_finished") => {
                    state_event("hito_plugin_torbox::journal", "torbox_journal");
                    let (operation, outcome) = if name == "torbox_journal_open_finished" {
                        ("journal_open", "ready")
                    } else {
                        ("journal_close", "joined")
                    };
                    assert_eq!(event["span"]["operation"], operation);
                    assert_eq!(fields["outcome"], outcome);
                    assert_eq!(fields["count"], 0);
                    lifecycle.push(operation);
                }
                "torbox_journal_command_finished" => {
                    state_event("hito_plugin_torbox::journal", "torbox_journal");
                    let operation = event["span"]["operation"].as_str().unwrap();
                    assert!(matches!(
                        operation,
                        "receipt_admission"
                            | "receipt_status"
                            | "candidate_current"
                            | "candidate_snapshot"
                            | "mint_intent"
                            | "receipt_completion"
                    ));
                    match fields["outcome"].as_str().unwrap() {
                        "complete" => {
                            assert_eq!(fields["count"], 1);
                            completed_commands += 1;
                        }
                        "rejected" => {
                            assert_eq!(operation, "receipt_status");
                            assert_eq!(fields["count"], 0);
                            rejected_commands += 1;
                        }
                        _ => panic!("an unqualified journal command outcome was captured"),
                    }
                }
                "torbox_journal_transition_committed" => {
                    state_event(
                        "hito_plugin_torbox::journal::transactions",
                        "torbox_journal",
                    );
                    assert_eq!(fields["count"], 1);
                    transitions.push((
                        event["span"]["operation"].as_str().unwrap(),
                        fields["outcome"].as_str().unwrap(),
                    ));
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
                "provider_completed",
                "provider_completed"
            ]
        );
        assert_eq!(preparations, ["incomplete", "candidate"]);
        assert!(opening.is_some() && opening == body);
        assert_eq!(
            persistence,
            [
                "candidate_authority",
                "candidate_sealing",
                "candidate_restoration"
            ]
        );
        assert_eq!(lifecycle, ["journal_open", "journal_close"]);
        assert_eq!((completed_commands, rejected_commands), (17, 1));
        assert_eq!(
            transitions,
            [
                ("receipt_admission", "checking"),
                ("receipt_completion", "preparing"),
                ("receipt_status", "checking"),
                ("mint_intent", "mint_intent"),
                ("receipt_completion", "ready"),
            ]
        );
        assert_eq!(events.len(), 54);
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
