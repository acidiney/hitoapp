#![cfg(feature = "device")]
#[path = "fixtures/application.rs"]
mod application;
use application::Observation;
use futures_util::future::BoxFuture;
use hito_audio::test_support::{self as audio, capture};
use hito_desktop::{RemoteHost, ServerSession, current_server};
use hito_server::{HostPlace, HostPlaceCurrent};
use reqwest::{Client, Method, Response};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;
use url::Url;
use uuid::Uuid;

struct Chosen(Url);

impl HostPlace for Chosen {
    fn current(&self) -> HostPlaceCurrent {
        current_server(&self.0, Some("listener".into()))
    }

    fn change(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }

    fn forget_me(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct Kept(Mutex<Option<String>>);

impl ServerSession for Kept {
    fn token(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }

    fn keep(&self, token: &str, _username: &str) -> bool {
        *self.0.lock().unwrap() = Some(token.to_owned());
        true
    }

    fn end(&self) {
        *self.0.lock().unwrap() = None;
    }
}

struct Server {
    _directory: tempfile::TempDir,
    origin: Url,
    client: Client,
    token: String,
    recording: String,
}

impl Server {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("web/assets")).unwrap();
        std::fs::write(directory.path().join("web/assets/app.js"), "export {};\n").unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let mut wave = audio::wave(48_000, 1, 480_000, |frame, _| {
            ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
        });
        let mut bytes = Vec::new();
        wave.read_to_end(&mut bytes).unwrap();
        std::fs::write(directory.path().join("music/owned.wav"), bytes).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let mut config = hito_server::RuntimeConfig::load(
            directory.path().join("hito.db"),
            directory.path().join("music"),
            directory.path().join("web"),
            None,
        )
        .await
        .unwrap();
        config.http.origin = origin.origin().ascii_serialization();
        let services = hito_core::open(config.core).await.unwrap();
        let app = hito_server::router_with_config(services.clone(), config.http);
        tokio::spawn(hito_server::serve(
            listener,
            app,
            services,
            std::future::pending(),
        ));
        let mut server = Self {
            _directory: directory,
            origin,
            client: Client::new(),
            token: String::new(),
            recording: String::new(),
        };
        let session = server
            .call(
                Method::POST,
                "api/v1/setup",
                json!({"username": "listener", "password": "private remote desktop password"}),
            )
            .await;
        server.token = session["token"].as_str().unwrap().into();
        let libraries = server
            .call(Method::GET, "api/v1/libraries", Value::Null)
            .await;
        let library = libraries[0]["id"].as_str().unwrap().to_owned();
        let jobs = server
            .call(
                Method::POST,
                &format!("api/v1/libraries/{library}/scan"),
                Value::Null,
            )
            .await;
        let job = jobs[0]["id"].as_str().unwrap().to_owned();
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let state = server
                    .call(Method::GET, &format!("api/v1/jobs/{job}"), Value::Null)
                    .await;
                if state["state"] == "complete" {
                    break;
                }
                assert_ne!(state["state"], "failed");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let recordings = server
            .call(Method::GET, "api/v1/recordings", Value::Null)
            .await;
        server.recording = recordings["items"][0]["id"].as_str().unwrap().into();
        server
    }

    async fn call(&self, method: Method, path: &str, body: Value) -> Value {
        let mut request = self
            .client
            .request(method, self.origin.join(path).unwrap())
            .header("origin", self.origin.origin().ascii_serialization())
            .header("x-hito-request", "1")
            .json(&body);
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        let response = request.send().await.unwrap();
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
        if response.content_length() == Some(0) {
            return Value::Null;
        }
        response.json().await.unwrap_or(Value::Null)
    }
}

struct Window {
    client: Client,
    root: Url,
    cookie: String,
}

impl Window {
    async fn open(host: &RemoteHost) -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let bootstrap = client.get(host.entry_url()).send().await.unwrap();
        let cookie = bootstrap.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        Self {
            client,
            root: host.entry_url().join("/").unwrap(),
            cookie,
        }
    }

    async fn request(&self, path: &str, body: Value) -> Response {
        let method = if body.is_null() {
            Method::GET
        } else {
            Method::POST
        };
        self.client
            .request(method, self.root.join(path).unwrap())
            .header("cookie", &self.cookie)
            .header("origin", self.root.origin().ascii_serialization())
            .header("x-hito-request", "1")
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn call(&self, path: &str, body: Value) -> Value {
        let response = self.request(path, body).await;
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
        response.json().await.unwrap()
    }

    async fn command(&self, lease: &Value, sequence: u64, revision: u64, action: Value) -> Value {
        self.call(
            "api/v1/player/commands",
            json!({"id": Uuid::new_v4(), "lease": lease, "sequence": sequence, "revision": revision, "action": action}),
        )
        .await
    }

    async fn observe(&self, lease: &Value, kind: &str) -> Observation {
        let response = self
            .request(
                &format!("api/v1/player/{kind}/events"),
                json!({"lease": lease}),
            )
            .await;
        assert!(response.status().is_success());
        Observation::new(response)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_remote_library_plays_through_the_native_player_until_the_server_withdraws_it() {
    let server = Server::start().await;
    let web = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(web.path().join("assets")).unwrap();
    let session = Arc::new(Kept(Mutex::new(Some(server.token.clone()))));
    let host = RemoteHost::start_native(
        server.origin.clone(),
        web.path(),
        Arc::new(Chosen(server.origin.clone())),
        session,
        None,
    )
    .await
    .unwrap();
    let window = Window::open(&host).await;

    let inspected = window.call("api/v1/player", Value::Null).await;
    assert_eq!(inspected["native"], true);
    let attached = window
        .call("api/v1/player/claim", json!({"epoch": inspected["epoch"]}))
        .await;
    let lease = &attached["lease"];
    let mut state = window.observe(lease, "state").await;
    let capture = capture::Capture::open();
    window
        .command(
            lease,
            1,
            0,
            json!({"kind": "replace", "recordings": [server.recording], "start": 0}),
        )
        .await;
    window.command(lease, 2, 1, json!({"kind": "resume"})).await;
    state.until(|state| state["playing"] == true).await;
    capture.after(capture.mark(), 0.3);
    assert!(capture::amplitude(&capture.recent(0.1), 440.) > 0.1);

    window.command(lease, 3, 1, json!({"kind": "pause"})).await;
    state
        .until(|state| state["playing"] == false && state["desired_playing"] == false)
        .await;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let saved = server
                .call(
                    Method::POST,
                    "api/v1/recordings/progress",
                    json!({"recording_ids": [server.recording]}),
                )
                .await;
            if saved[0]["position_ms"].as_u64().unwrap_or(0) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the native player saved listening progress on the server");

    window.command(lease, 4, 1, json!({"kind": "resume"})).await;
    state.until(|state| state["playing"] == true).await;
    server
        .call(Method::POST, "api/v1/logout", Value::Null)
        .await;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            capture.after(capture.mark(), 0.2);
            if capture::energy(&capture.recent(0.1)) < 0.000001 {
                break;
            }
        }
    })
    .await
    .expect("playback stopped once the server ended the session");

    host.shutdown().await.unwrap();
}
