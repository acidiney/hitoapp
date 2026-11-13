use axum::http::StatusCode;
use hito_plugin_sdk::{
    Capability, DETAIL_PATH, MANIFEST_PATH, Manifest, PROTOCOL_VERSION, RECORDING_LOOKUP_PATH,
    RecordingLookupRequest, SOURCES_PATH,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin, Gate, RecordedRequest};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const REQUEST_LIMIT: usize = 64;
pub const KNOWN_REFERENCE: &str = "https://links.example/owned/known-recording";
pub const KNOWN_RECORDING: &str = "owned-canonical-recording";

#[derive(Clone)]
pub enum Answer {
    Recording(Value),
    Raw(Value),
    Status(StatusCode),
}

pub struct SourceLinkServer {
    pub url: String,
    id: String,
    plugin: FakePlugin,
    answers: Arc<Mutex<HashMap<String, Answer>>>,
}

pub fn recording(id: &str) -> Value {
    json!({
        "id": id, "title": format!("Owned linked title {id}"), "artist": "Owned linked artist",
        "duration_ms": 61_000, "release": null, "disc_number": null, "track_number": null,
        "revision": null
    })
}

impl SourceLinkServer {
    pub async fn start() -> Self {
        Self::with_capabilities(
            "org.hito.source-link-fixture",
            [
                Capability::Search,
                Capability::Sources,
                Capability::SourceLinks,
            ],
        )
        .await
    }

    pub async fn without_source_links() -> Self {
        Self::with_capabilities(
            "org.hito.no-source-link-fixture",
            [Capability::Search, Capability::Sources],
        )
        .await
    }

    pub async fn with_capabilities(
        id: &str,
        capabilities: impl IntoIterator<Item = Capability>,
    ) -> Self {
        let answers: Arc<Mutex<HashMap<String, Answer>>> = Arc::new(Mutex::new(HashMap::from([(
            KNOWN_REFERENCE.to_owned(),
            Answer::Recording(recording(KNOWN_RECORDING)),
        )])));
        let plugin = FakePlugin::builder(Manifest {
            id: id.into(),
            name: "Owned source link service".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: capabilities.into_iter().collect(),
            authorization: None,
            content_policy: None,
        })
        .post(RECORDING_LOOKUP_PATH, {
            let answers = answers.clone();
            move |request| lookup(&answers, request)
        })
        .post(DETAIL_PATH, |request| {
            let id = request.json()["recording_id"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned();
            fake::Reply::Json(recording(&id))
        })
        .post(SOURCES_PATH, |request| {
            let id = request.json()["recording_id"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned();
            fake::Reply::Json(json!({"items": [{
                "id": format!("{id}/audio"), "label": "Owned linked source", "ready": true
            }]}))
        })
        .strict(REQUEST_LIMIT)
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            id: id.to_owned(),
            plugin,
            answers,
        }
    }

    pub fn declare(&self, capabilities: impl IntoIterator<Item = Capability>) {
        let manifest = Manifest {
            id: self.id.clone(),
            name: "Owned source link service".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: capabilities.into_iter().collect(),
            authorization: None,
            content_policy: None,
        };
        self.plugin.reply(
            MANIFEST_PATH,
            fake::Reply::Json(serde_json::to_value(manifest).unwrap()),
        );
    }

    pub fn answer(&self, reference: &str, answer: Answer) {
        self.answers
            .lock()
            .unwrap()
            .insert(reference.to_owned(), answer);
    }

    pub fn hold_lookup(&self) -> Arc<Gate> {
        self.plugin.hold(RECORDING_LOOKUP_PATH)
    }

    pub fn stall_lookup_body(&self, prefix: usize) -> Arc<Gate> {
        self.plugin.stall_body(RECORDING_LOOKUP_PATH, prefix)
    }

    pub fn lookup_references(&self) -> Vec<Value> {
        self.plugin
            .requests_for(RECORDING_LOOKUP_PATH)
            .into_iter()
            .map(|request| request.json())
            .collect()
    }

    pub fn lookup_requests(&self) -> Vec<RecordedRequest> {
        self.plugin.requests_for(RECORDING_LOOKUP_PATH)
    }

    pub fn detail_requests(&self) -> usize {
        self.plugin.requests_for(DETAIL_PATH).len()
    }

    pub fn source_requests(&self) -> usize {
        self.plugin.requests_for(SOURCES_PATH).len()
    }

    pub fn unexpected(&self) -> usize {
        self.plugin.unexpected()
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}

fn lookup(answers: &Mutex<HashMap<String, Answer>>, request: &RecordedRequest) -> fake::Reply {
    let Some(parsed) = serde_json::from_slice::<RecordingLookupRequest>(&request.body)
        .ok()
        .filter(|parsed| parsed.validate().is_ok())
    else {
        return fake::Reply::Refuse;
    };
    match answers.lock().unwrap().get(&parsed.reference).cloned() {
        Some(Answer::Recording(recording)) => {
            let source = format!("{}/audio", recording["id"].as_str().unwrap_or("unknown"));
            fake::Reply::Json(json!({"recording": recording, "source_ids": [source]}))
        }
        Some(Answer::Raw(value)) => fake::Reply::Json(value),
        Some(Answer::Status(status)) => fake::Reply::Status(status),
        None => fake::Reply::Status(StatusCode::NOT_FOUND),
    }
}
