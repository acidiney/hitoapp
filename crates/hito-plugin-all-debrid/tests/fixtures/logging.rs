use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use uuid::Uuid;

#[path = "recovery_logging.rs"]
mod recovery;
#[path = "retry_logging.rs"]
mod retry;

#[derive(Clone, Default)]
pub struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
    overflow: Arc<AtomicBool>,
}

impl Capture {
    pub fn dispatch(&self) -> tracing::Dispatch {
        let capture = self.clone();
        tracing::Dispatch::new(tracing_subscriber::fmt()
            .with_env_filter("off,hito_plugin_all_debrid=trace,hito_debrid_all_debrid=trace,hito_remote_audio=trace")
            .without_time().with_ansi(false).json().with_current_span(true).with_span_list(true)
            .with_writer(move || capture.clone()).finish())
    }

    pub fn assert_workflow(&self, statuses: &[u16], private: &[&str], length: u64) {
        assert_eq!(statuses.len(), 22);
        assert!(!self.overflow.load(Ordering::SeqCst));
        let bytes = self.bytes.lock().unwrap();
        assert!(!bytes.is_empty() && bytes.len() <= 65_536);
        let text = std::str::from_utf8(&bytes).unwrap();
        for secret in private.iter().copied().chain([
            "http://",
            "https://",
            "http%3A",
            "https%3A",
            "Bearer ",
            "owned-private-media-cookie",
            "owned-private-upstream-name.wav",
        ]) {
            assert!(!secret.is_empty());
            let escaped = serde_json::to_string(secret).unwrap();
            assert!(!text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]));
        }
        let events = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let mut requests = BTreeMap::new();
        let mut actual_statuses = Vec::new();
        let mut counts = BTreeMap::<&str, usize>::new();
        let mut opening = None;
        let mut body_owner = None;
        let mut provider_ids = BTreeSet::new();
        let mut observations = BTreeMap::<&str, usize>::new();
        for event in &events {
            keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            let fields = &event["fields"];
            let name = fields["event"].as_str().unwrap();
            let target = event["target"].as_str().unwrap();
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let spans = event["spans"].as_array().unwrap();
            assert!(!spans.is_empty());
            assert_eq!(&event["span"], spans.last().unwrap());
            for span in spans {
                keys(span, &["name", "operation", "request_id"]);
                assert_eq!(
                    Uuid::parse_str(span["request_id"].as_str().unwrap())
                        .unwrap()
                        .get_version_num(),
                    4
                );
                let operation = span["operation"].as_str().unwrap();
                assert_eq!(
                    span["name"],
                    match operation {
                        "manifest" | "search" | "detail" | "sources" | "prepare"
                        | "preparation_status" | "audio" => "all_debrid_plugin_request",
                        "bound_file_preparation" => "all_debrid_preparation",
                        "delayed_file_poll" => "all_debrid_delayed_poll",
                        "magnet_observation" => "all_debrid_observation",
                        "bound_audio_opening" => "all_debrid_audio",
                        _ => panic!("only fixed safe workflow operations are allowed"),
                    }
                );
            }
            let owner = spans
                .iter()
                .find(|span| span["name"] == "all_debrid_plugin_request")
                .unwrap();
            let owner_id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            *counts.entry(name).or_default() += 1;
            match name {
                "all_debrid_plugin_request_finished" => {
                    assert_eq!(target, "hito_plugin_all_debrid::http");
                    keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                    let status = fields["status"].as_u64().unwrap() as u16;
                    assert_eq!(
                        fields["outcome"],
                        match status {
                            200 => "success",
                            401 => "unauthorized",
                            404 => "rejected",
                            _ => panic!("the public workflow returned an unexpected HTTP outcome"),
                        }
                    );
                    assert!(
                        requests
                            .insert(owner_id, owner["operation"].as_str().unwrap())
                            .is_none()
                    );
                    actual_statuses.push(status);
                }
                "all_debrid_plugin_audio_finished" => {
                    assert_eq!(target, "hito_plugin_all_debrid::audio");
                    keys(fields, &["event", "outcome", "bytes", "elapsed_ms"]);
                    assert_eq!(fields["outcome"], "complete");
                    assert_eq!(fields["bytes"].as_u64(), Some(length));
                    assert_eq!(owner["operation"], "audio");
                }
                "remote_audio_body_finished" => {
                    assert_eq!(target, "hito_remote_audio::body");
                    keys(
                        fields,
                        &["event", "message", "outcome", "bytes", "elapsed_ms"],
                    );
                    assert_eq!(fields["message"], "Remote audio body finished");
                    assert_eq!(fields["outcome"], "complete");
                    assert_eq!(fields["bytes"].as_u64(), Some(length));
                    body_owner = Some(event["span"]["request_id"].as_str().unwrap());
                }
                "all_debrid_preparation_finished"
                | "all_debrid_delayed_poll_finished"
                | "all_debrid_observation_finished"
                | "all_debrid_audio_opening_finished" => {
                    assert_eq!(target, "hito_debrid_all_debrid::client");
                    keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                    assert_eq!(fields["count"].as_u64(), Some(1));
                    assert_eq!(
                        fields["outcome"],
                        match name {
                            "all_debrid_preparation_finished" => "delayed",
                            "all_debrid_delayed_poll_finished" => "candidate",
                            "all_debrid_observation_finished" => "provider_completed",
                            _ => "complete",
                        }
                    );
                    let id = event["span"]["request_id"].as_str().unwrap();
                    assert!(provider_ids.insert(id));
                    if name == "all_debrid_audio_opening_finished" {
                        opening = Some(id);
                    }
                    if name == "all_debrid_observation_finished" {
                        *observations
                            .entry(owner["operation"].as_str().unwrap())
                            .or_default() += 1;
                    }
                }
                _ => panic!("an unrecognized workflow event must not bypass the privacy oracle"),
            }
        }
        assert_eq!(actual_statuses, statuses);
        assert_eq!(requests.len(), 22);
        assert_eq!(opening, body_owner);
        assert_eq!(provider_ids.len(), 6);
        assert_eq!(
            observations,
            BTreeMap::from([("audio", 1), ("preparation_status", 1), ("sources", 1)])
        );
        assert_eq!(
            counts,
            BTreeMap::from([
                ("all_debrid_plugin_request_finished", 22),
                ("all_debrid_plugin_audio_finished", 1),
                ("all_debrid_preparation_finished", 1),
                ("all_debrid_delayed_poll_finished", 1),
                ("all_debrid_observation_finished", 3),
                ("all_debrid_audio_opening_finished", 1),
                ("remote_audio_body_finished", 1),
            ])
        );
        for event in &events {
            let owner = event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|span| span["name"] == "all_debrid_plugin_request")
                .unwrap();
            let id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            assert!(requests.contains_key(&id));
        }
    }
}

fn keys(value: &Value, expected: &[&str]) {
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        expected.iter().copied().collect()
    );
}

impl Write for Capture {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut bytes = self.bytes.lock().unwrap();
        if input.len() > 65_536_usize.saturating_sub(bytes.len()) {
            self.overflow.store(true, Ordering::SeqCst);
            return Err(io::Error::other(
                "owned workflow log capture exceeded its bound",
            ));
        }
        bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    pub fn assert_retarget_rejected(&self, private: &[&str]) {
        assert!(!self.overflow.load(Ordering::SeqCst));
        let bytes = self.bytes.lock().unwrap();
        assert!(!bytes.is_empty() && bytes.len() <= 65_536);
        let text = std::str::from_utf8(&bytes).unwrap();
        for secret in private
            .iter()
            .copied()
            .chain(["http://", "https://", "http%3A", "https%3A", "Bearer "])
        {
            assert!(!secret.is_empty());
            let escaped = serde_json::to_string(secret).unwrap();
            assert!(!text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]));
        }
        let events = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 5);
        let mut requests = BTreeSet::new();
        let mut owners = Vec::new();
        let mut statuses = Vec::new();
        let mut operations = Vec::new();
        let mut preparations = 0;
        for event in &events {
            keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            let spans = event["spans"].as_array().unwrap();
            assert_eq!(&event["span"], spans.last().unwrap());
            for span in spans {
                keys(span, &["name", "operation", "request_id"]);
                assert_eq!(
                    Uuid::parse_str(span["request_id"].as_str().unwrap())
                        .unwrap()
                        .get_version_num(),
                    4
                );
            }
            let fields = &event["fields"];
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let owner = &spans[0];
            assert_eq!(owner["name"], "all_debrid_plugin_request");
            let id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            owners.push(id);
            match fields["event"].as_str().unwrap() {
                "all_debrid_plugin_request_finished" => {
                    assert_eq!(event["target"], "hito_plugin_all_debrid::http");
                    assert_eq!(spans.len(), 1);
                    keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                    let status = fields["status"].as_u64().unwrap();
                    assert_eq!(
                        fields["outcome"],
                        if status == 200 { "success" } else { "rejected" }
                    );
                    assert!(requests.insert(id));
                    statuses.push(status);
                    operations.push(owner["operation"].as_str().unwrap());
                }
                "all_debrid_preparation_finished" => {
                    assert_eq!(event["target"], "hito_debrid_all_debrid::client");
                    assert_eq!(spans.len(), 2);
                    assert_eq!(owner["operation"], "prepare");
                    assert_eq!(spans[1]["name"], "all_debrid_preparation");
                    assert_eq!(spans[1]["operation"], "bound_file_preparation");
                    assert_ne!(spans[1]["request_id"], owner["request_id"]);
                    keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                    assert_eq!(fields["outcome"], "delayed");
                    assert_eq!(fields["count"].as_u64(), Some(1));
                    preparations += 1;
                }
                _ => panic!("no other workflow may bypass the operation-conflict privacy oracle"),
            }
        }
        assert_eq!(statuses, [200, 409, 200, 200]);
        assert_eq!(operations, ["prepare", "prepare", "prepare", "sources"]);
        assert_eq!(preparations, 1);
        assert_eq!(requests.len(), 4);
        assert!(owners.iter().all(|id| requests.contains(id)));
    }
}

impl Capture {
    pub fn assert_processing_receipt(&self, private: &[&str]) {
        assert!(!self.overflow.load(Ordering::SeqCst));
        let bytes = self.bytes.lock().unwrap();
        assert!(!bytes.is_empty() && bytes.len() <= 65_536);
        let text = std::str::from_utf8(&bytes).unwrap();
        for secret in private
            .iter()
            .copied()
            .chain(["http://", "https://", "http%3A", "https%3A", "Bearer "])
        {
            assert!(!secret.is_empty());
            let escaped = serde_json::to_string(secret).unwrap();
            assert!(!text.contains(secret) && !text.contains(&escaped[1..escaped.len() - 1]));
        }
        let events = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 11);
        let mut requests = BTreeSet::new();
        let mut owners = Vec::new();
        let mut statuses = Vec::new();
        let mut operations = Vec::new();
        let mut provider = BTreeMap::<&str, usize>::new();
        let mut provider_ids = BTreeSet::new();
        for event in &events {
            keys(event, &["level", "fields", "target", "span", "spans"]);
            assert_eq!(event["level"], "DEBUG");
            let spans = event["spans"].as_array().unwrap();
            assert_eq!(&event["span"], spans.last().unwrap());
            for span in spans {
                keys(span, &["name", "operation", "request_id"]);
                assert_eq!(
                    Uuid::parse_str(span["request_id"].as_str().unwrap())
                        .unwrap()
                        .get_version_num(),
                    4
                );
            }
            let fields = &event["fields"];
            assert!(fields["elapsed_ms"].as_u64().is_some());
            let owner = &spans[0];
            assert_eq!(owner["name"], "all_debrid_plugin_request");
            let id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            owners.push(id);
            let name = fields["event"].as_str().unwrap();
            if name == "all_debrid_plugin_request_finished" {
                assert_eq!(event["target"], "hito_plugin_all_debrid::http");
                assert_eq!(spans.len(), 1);
                keys(fields, &["event", "outcome", "status", "elapsed_ms"]);
                assert_eq!(fields["outcome"], "success");
                statuses.push(fields["status"].as_u64().unwrap());
                operations.push(owner["operation"].as_str().unwrap());
                assert!(requests.insert(id));
            } else {
                assert_eq!(event["target"], "hito_debrid_all_debrid::client");
                assert_eq!(spans.len(), 2);
                let (span_name, operation, outcome) = match name {
                    "all_debrid_preparation_finished" => (
                        "all_debrid_preparation",
                        "bound_file_preparation",
                        "delayed",
                    ),
                    "all_debrid_delayed_poll_finished" => {
                        ("all_debrid_delayed_poll", "delayed_file_poll", "candidate")
                    }
                    "all_debrid_observation_finished" => {
                        ("all_debrid_observation", "magnet_observation", "processing")
                    }
                    _ => panic!("no other workflow may bypass current-readiness validation"),
                };
                assert_eq!(spans[1]["name"], span_name);
                assert_eq!(spans[1]["operation"], operation);
                assert_ne!(spans[1]["request_id"], owner["request_id"]);
                assert!(provider_ids.insert(spans[1]["request_id"].as_str().unwrap()));
                keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                assert_eq!(fields["outcome"], outcome);
                assert_eq!(fields["count"].as_u64(), Some(1));
                *provider.entry(name).or_default() += 1;
            }
        }
        assert_eq!(statuses, [200; 5]);
        assert_eq!(
            operations,
            [
                "prepare",
                "preparation_status",
                "sources",
                "prepare",
                "preparation_status"
            ]
        );
        assert_eq!(requests.len(), 5);
        assert_eq!(provider_ids.len(), 6);
        assert_eq!(
            provider,
            BTreeMap::from([
                ("all_debrid_preparation_finished", 1),
                ("all_debrid_delayed_poll_finished", 1),
                ("all_debrid_observation_finished", 4),
            ])
        );
        assert!(owners.iter().all(|id| requests.contains(id)));
    }
}
