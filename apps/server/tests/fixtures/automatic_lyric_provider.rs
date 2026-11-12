use crate::lrclib::{ARTIST, TITLE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use hito_plugin_sdk::Manifest;
use hito_testkit::fake_plugin::{FakePlugin, Gate, Handled, RecordedRequest, Reply};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone)]
struct Response {
    status: StatusCode,
    bytes: Vec<u8>,
    chunked: bool,
}
impl Response {
    fn json(value: Value) -> Self {
        Self {
            status: StatusCode::OK,
            bytes: serde_json::to_vec(&value).unwrap(),
            chunked: false,
        }
    }
}
struct Data {
    replies: HashMap<&'static str, Response>,
    request_replies: HashMap<(&'static str, String), Response>,
    query: String,
}
pub struct HeldBody {
    gate: Arc<Gate>,
    released: AtomicBool,
}
impl HeldBody {
    pub async fn entered(&self) {
        self.gate.entered().await;
    }
    pub async fn retired(&self) {
        self.gate.retired().await;
    }
    pub fn released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }
}
pub struct Provider {
    plugin: FakePlugin,
    state: Arc<Mutex<Data>>,
    held: Mutex<Option<(&'static str, Arc<HeldBody>)>>,
}
pub fn recording() -> Value {
    json!({"id":"owned:automatic","title":TITLE,"artist":ARTIST,"duration_ms":1000,"release":null,"disc_number":null,"track_number":null,"revision":"owned:1"})
}
pub fn reference() -> Value {
    json!({"id":"owned:automatic","revision":"owned:1"})
}
pub fn response() -> Value {
    json!({"recording":reference(),"lyrics":{"id":"owned:auto-doc","revision":"1","rendition_id":"owned:auto-rendition","content":{"kind":"plain","lines":["Owned automatic protocol text"]}}})
}
impl Provider {
    pub async fn start() -> Self {
        let manifest = json!({"id":"org.hito.owned-automatic-lyrics","name":"Owned automatic lyrics","protocol_version":1,"capabilities":["search","metadata","lyrics"]});
        let state = Arc::new(Mutex::new(Data {
            query: format!("{ARTIST} {TITLE}"),
            request_replies: HashMap::new(),
            replies: HashMap::from([
                ("manifest", Response::json(manifest.clone())),
                (
                    "search",
                    Response::json(json!({"items":[recording()],"next_cursor":null})),
                ),
                ("metadata", Response::json(json!({"recording":recording()}))),
                ("v2_descriptor", Response::json(json!({"lyrics_version":2}))),
                ("v1_descriptor", Response::json(json!({"lyrics_version":1}))),
                ("v2_lyrics", Response::json(response())),
                (
                    "v1_lyrics",
                    Response::json(json!({"recording":reference(),"lyrics":null})),
                ),
            ]),
        }));
        let manifest: Manifest = serde_json::from_value(manifest).unwrap();
        let mut builder = FakePlugin::builder(manifest).require_protocol().strict(128);
        for operation in [
            "search",
            "metadata",
            "v2_descriptor",
            "v1_descriptor",
            "v2_lyrics",
            "v1_lyrics",
        ] {
            let handler = handler(operation, state.clone());
            builder = if operation.ends_with("descriptor") {
                builder.get(path(operation), handler)
            } else {
                builder.post(path(operation), handler)
            };
        }
        let plugin = builder.start().await;
        plugin.on(path("manifest"), handler("manifest", state.clone()));
        plugin.set_hold_limit(Duration::from_secs(20));
        Self {
            plugin,
            state,
            held: Mutex::new(None),
        }
    }
    pub fn url(&self) -> &str {
        &self.plugin.url
    }
    pub fn require_bearer(&self, token: &str) {
        self.plugin.require_bearer(token);
    }
    pub fn query(&self, query: String) {
        self.state.lock().unwrap().query = query;
    }
    pub fn reply(&self, operation: &'static str, value: Value) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(operation, Response::json(value));
    }
    pub fn reply_for(&self, operation: &'static str, request: Value, value: Value) {
        assert!(matches!(operation, "metadata" | "v2_lyrics" | "v1_lyrics"));
        self.state.lock().unwrap().request_replies.insert(
            (operation, serde_json::to_string(&request).unwrap()),
            Response::json(value),
        );
    }
    pub fn status_for(&self, operation: &'static str, request: Value, status: StatusCode) {
        self.state
            .lock()
            .unwrap()
            .request_replies
            .get_mut(&(operation, serde_json::to_string(&request).unwrap()))
            .unwrap()
            .status = status;
    }
    pub fn requests_for(&self, operation: &'static str) -> Vec<Value> {
        self.plugin
            .requests_for(path(operation))
            .iter()
            .map(RecordedRequest::json)
            .collect()
    }
    pub fn status(&self, operation: &'static str, status: StatusCode) {
        self.state
            .lock()
            .unwrap()
            .replies
            .get_mut(operation)
            .unwrap()
            .status = status;
    }
    pub fn bytes(&self, operation: &'static str, bytes: Vec<u8>) {
        assert!(bytes.len() <= 524_288);
        let mut data = self.state.lock().unwrap();
        let response = data.replies.get_mut(operation).unwrap();
        response.bytes = bytes;
        response.chunked = true;
    }
    pub fn calls(&self) -> Vec<&'static str> {
        self.plugin
            .requests()
            .iter()
            .map(|request| operation(&request.operation))
            .collect()
    }
    pub fn hold(&self, operation: &'static str) -> Arc<HeldBody> {
        let mut held = self.held.lock().unwrap();
        assert!(held.is_none());
        let gate = Arc::new(HeldBody {
            gate: self.plugin.stall_body(path(operation), 1),
            released: AtomicBool::new(false),
        });
        *held = Some((operation, gate.clone()));
        gate
    }
    pub fn release(&self) {
        if let Some((operation, gate)) = self.held.lock().unwrap().take() {
            self.plugin.clear_fault(path(operation));
            gate.released.store(true, Ordering::SeqCst);
            gate.gate.open();
        }
    }
    pub fn advance_hold(&self, next_operation: &'static str) -> Arc<HeldBody> {
        let mut held = self.held.lock().unwrap();
        let (operation, previous) = held.take().unwrap();
        assert_ne!(operation, next_operation);
        let gate = Arc::new(HeldBody {
            gate: self.plugin.stall_body(path(next_operation), 1),
            released: AtomicBool::new(false),
        });
        self.plugin.clear_fault(path(operation));
        *held = Some((next_operation, gate.clone()));
        previous.released.store(true, Ordering::SeqCst);
        previous.gate.open();
        gate
    }
    pub async fn finish(mut self) {
        self.release();
        assert!(
            self.plugin
                .requests()
                .iter()
                .all(|request| request.handled == Handled::Yes),
            "Every declared protocol request must be accepted by the owned fixture"
        );
        assert!(
            self.plugin.finish().await,
            "The owned protocol fixture must shut down cleanly"
        );
    }
}
fn path(operation: &str) -> &'static str {
    match operation {
        "manifest" => "v1/manifest",
        "search" => "v1/search",
        "metadata" => "v1/recordings/metadata",
        "v2_descriptor" => "v2/lyrics/capabilities",
        "v1_descriptor" => "v1/lyrics/capabilities",
        "v2_lyrics" => "v2/lyrics",
        "v1_lyrics" => "v1/lyrics",
        _ => panic!("Unknown owned automatic lyric operation"),
    }
}
fn operation(path: &str) -> &'static str {
    match path {
        "v1/manifest" => "manifest",
        "v1/search" => "search",
        "v1/recordings/metadata" => "metadata",
        "v2/lyrics/capabilities" => "v2_descriptor",
        "v1/lyrics/capabilities" => "v1_descriptor",
        "v2/lyrics" => "v2_lyrics",
        "v1/lyrics" => "v1_lyrics",
        _ => panic!("Unexpected owned automatic lyric operation"),
    }
}
fn handler(
    operation: &'static str,
    state: Arc<Mutex<Data>>,
) -> impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static {
    move |request| {
        assert_eq!(request.protocol().as_deref(), Some("1"));
        assert!(request.query.is_none());
        let data = state.lock().unwrap();
        let keyed_reply = matches!(operation, "metadata" | "v2_lyrics" | "v1_lyrics")
            .then(|| {
                data.request_replies
                    .get(&(operation, serde_json::to_string(&request.json()).unwrap()))
            })
            .flatten();
        match operation {
            "search" => assert_eq!(
                request.json(),
                json!({"query":data.query,"cursor":null,"limit":20})
            ),
            "metadata" if keyed_reply.is_none() => {
                assert_eq!(request.json(), json!({"recording_id":"owned:automatic"}))
            }
            "v2_lyrics" | "v1_lyrics" if keyed_reply.is_none() => {
                assert_eq!(request.json(), json!({"recording":reference()}))
            }
            "metadata" | "v2_lyrics" | "v1_lyrics" => {}
            _ => assert!(request.body.is_empty()),
        }
        let response = keyed_reply.unwrap_or(&data.replies[operation]);
        let headers = HeaderMap::from_iter([(
            "content-type".parse().unwrap(),
            HeaderValue::from_static("application/json"),
        )]);
        Reply::Raw {
            status: response.status,
            headers,
            body: response.bytes.clone(),
            chunked: response.chunked,
        }
    }
}
