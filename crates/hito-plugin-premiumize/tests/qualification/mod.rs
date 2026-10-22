#[path = "../fixtures/http.rs"]
pub mod http;
#[path = "../fixtures/provider.rs"]
pub mod provider;
#[path = "../fixtures/wave_audio.rs"]
pub mod wave_fixture;
pub use hito_debrid_premiumize::test_support::wire_fixture;

use futures_util::{FutureExt, future};
use hito_plugin_premiumize::{Config, CuratedWavRecording, router};
use hito_testkit::logs::{LogCapture, trace_filter};
use provider::{Provider, *};
use reqwest::{
    Client, Method, StatusCode,
    header::{HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet, future::Future, panic::AssertUnwindSafe, pin::Pin, time::Duration,
};
use tokio::sync::oneshot;
use url::Url;
use uuid::Uuid;
use wire_fixture::{Fixture, Response};

pub type Scenario<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

pub struct Case {
    provider: Option<Provider>,
    servers: Vec<http::Server>,
    client: Client,
    capture: Capture,
    requests: BTreeSet<Uuid>,
    private: Vec<String>,
    cleanup_release: Option<oneshot::Sender<()>>,
}

pub async fn run<F>(responses: Vec<Response>, scenario: F)
where
    F: for<'a> FnOnce(&'a mut Case) -> Scenario<'a>,
{
    run_media(
        responses,
        vec![Response::wave(wave_fixture::wav())],
        scenario,
    )
    .await;
}

pub async fn run_media<F>(responses: Vec<Response>, media_responses: Vec<Response>, scenario: F)
where
    F: for<'a> FnOnce(&'a mut Case) -> Scenario<'a>,
{
    let provider = Provider {
        api: Fixture::responses("www.premiumize.me", responses).await,
        media: Fixture::responses(MEDIA_HOST, media_responses).await,
        peer_api: Fixture::responses("www.premiumize.me", vec![transfer("queued", 0.0)]).await,
        peer_media: Fixture::responses(MEDIA_HOST, vec![Response::wave(wave_fixture::wav())]).await,
    };
    let mut case = Case {
        provider: Some(provider),
        servers: Vec::new(),
        capture: Capture::default(),
        client: Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap(),
        requests: BTreeSet::new(),
        private: Vec::new(),
        cleanup_release: None,
    };
    let outcome = AssertUnwindSafe(tokio::time::timeout(
        Duration::from_secs(30),
        scenario(&mut case),
    ))
    .catch_unwind()
    .await;
    if let Some(release) = case.cleanup_release.take() {
        let _ = release.send(());
    }
    let counts = case.counts();
    let closing = future::join_all(case.servers.drain(..).map(http::Server::close));
    let (stops, exchanges) = future::join(closing, case.provider.take().unwrap().close()).await;
    for stopped in stops {
        stopped.expect("an owned qualification HTTP server must stop");
    }
    assert_eq!(
        [
            exchanges.0.len(),
            exchanges.1.len(),
            exchanges.2.len(),
            exchanges.3.len()
        ],
        counts
    );
    match outcome {
        Ok(result) => result.expect("the owned qualification workflow exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
    case.capture.assert_safe(&case.requests, &case.private);
}

impl Case {
    pub fn config(&self) -> Config {
        Config {
            connections: self.provider.as_ref().unwrap().connections(),
        }
    }

    pub fn counts(&self) -> [usize; 4] {
        self.provider.as_ref().unwrap().counts()
    }

    pub fn protect(&mut self, value: &str) {
        self.private.push(value.to_owned());
    }

    pub fn release_after_outcome(&mut self, release: oneshot::Sender<()>) {
        assert!(self.cleanup_release.replace(release).is_none());
    }

    pub fn events(&self) -> Vec<Value> {
        self.capture.0.events()
    }

    pub fn record_response(&mut self, response: &reqwest::Response, expected: StatusCode) -> Uuid {
        assert_eq!(
            response.status(),
            expected,
            "the public qualification status differs"
        );
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let id = Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).unwrap();
        assert_eq!(id.get_version_num(), 4);
        assert!(self.requests.insert(id), "HTTP correlations must be fresh");
        id
    }

    pub async fn start(&mut self, config: Config) -> Url {
        let server = http::Server::start(
            router(config).expect("the owned configuration must initialize"),
            self.capture.dispatch(),
        )
        .await;
        let base = server.base.clone();
        self.servers.push(server);
        base
    }

    pub async fn stop(&mut self) {
        self.servers
            .pop()
            .unwrap()
            .close()
            .await
            .expect("the previous router must stop before restart");
    }

    pub async fn request(
        &mut self,
        base: &Url,
        method: Method,
        path: &str,
        headers: HeaderMap,
        body: Option<&str>,
        expected: StatusCode,
    ) -> Value {
        let mut request = self
            .client
            .request(method, base.join(path).unwrap())
            .headers(headers);
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body.to_owned());
        }
        let mut response = request
            .send()
            .await
            .expect("the owned HTTP request must complete");
        self.record_response(&response, expected);
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .expect("the owned JSON body must complete")
        {
            assert!(chunk.len() <= 65_536_usize.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk);
        }
        provider::assert_private_wire(&bytes);
        serde_json::from_slice(&bytes).expect("the public response must be JSON")
    }
}

pub fn headers(token: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-hito-plugin-protocol", HeaderValue::from_static("1"));
    headers.insert("cookie", HeaderValue::from_static(COOKIE));
    if let Some(token) = token {
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
    }
    headers
}

pub fn transfer(phase: &str, progress: f64) -> Response {
    let available = matches!(phase, "finished" | "seeding");
    Response::json(
        serde_json::to_vec(&json!({"status":"success", "transfers":[{
            "id":TRANSFER, "name":FILE, "created_at":TRANSFER_CREATED,
            "status":phase, "progress":progress,
            "file_id":if available { Some(ITEM) } else { None },
            "folder_id":if available { Some(FOLDER) } else { None }
        }]}))
        .unwrap(),
    )
}

pub fn sibling(entry: &CuratedWavRecording) -> CuratedWavRecording {
    let mut recording = entry.recording.clone();
    recording.id = "owned-private-second-recording".into();
    let mut binding = entry.binding.clone();
    binding.transfer.transfer_id = "owned-private-second-transfer".into();
    binding.item_id = "owned-private-second-item".into();
    CuratedWavRecording {
        recording,
        source_id: "owned-private-second-source".into(),
        label: entry.label.clone(),
        binding,
    }
}

pub fn copy(entry: &CuratedWavRecording) -> CuratedWavRecording {
    CuratedWavRecording {
        recording: entry.recording.clone(),
        source_id: entry.source_id.clone(),
        label: entry.label.clone(),
        binding: entry.binding.clone(),
    }
}

#[derive(Clone, Default)]
struct Capture(LogCapture);

impl Capture {
    fn dispatch(&self) -> tracing::Dispatch {
        self.0.dispatch_with_spans(&trace_filter(&[
            "hito_plugin_premiumize",
            "hito_debrid_premiumize",
            "hito_remote_audio",
        ]))
    }

    fn assert_safe(&self, requests: &BTreeSet<Uuid>, private: &[String]) {
        let text = self.0.text();
        for value in [
            "owned-private",
            "http://",
            "https://",
            "Bearer ",
            RECORDING,
            SOURCE,
            PEER_RECORDING,
            PEER_SOURCE,
        ]
        .into_iter()
        .chain(private.iter().map(String::as_str))
        {
            assert!(
                !text.contains(value),
                "a private value entered workflow logs"
            );
        }
        let mut completed = BTreeSet::new();
        for line in text.lines() {
            let event: Value = serde_json::from_str(line).unwrap();
            assert_eq!(event["level"], "DEBUG");
            assert!(event["fields"]["elapsed_ms"].as_u64().is_some());
            let owner = event["spans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|span| span["name"] == "premiumize_plugin_request")
                .unwrap();
            let id = Uuid::parse_str(owner["request_id"].as_str().unwrap()).unwrap();
            assert!(requests.contains(&id));
            if event["fields"]["event"] == "premiumize_plugin_request_finished" {
                assert!(completed.insert(id));
            }
        }
        assert_eq!(&completed, requests);
    }
}
