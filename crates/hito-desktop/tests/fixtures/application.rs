#![allow(dead_code)]
#[path = "../../../hito-audio/tests/support/mod.rs"]
mod audio;
use hito_desktop::{LocalHost, Paths};
use reqwest::{Client, Method, Response, Url};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};
use uuid::Uuid;

pub struct Application {
    pub directory: tempfile::TempDir,
    pub host: Option<LocalHost>,
    pub client: Client,
    pub root: Url,
    pub cookie: String,
    pub token: String,
    pub recording: String,
}
impl Application {
    pub async fn start() -> Self {
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
        let host = LocalHost::start(paths(directory.path())).await.unwrap();
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let bootstrap = client.get(host.entry_url()).send().await.unwrap();
        assert_eq!(bootstrap.status(), reqwest::StatusCode::SEE_OTHER);
        let cookie = bootstrap.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let root = host.entry_url().join("/").unwrap();
        let mut app = Self {
            directory,
            host: Some(host),
            client,
            root,
            cookie,
            token: String::new(),
            recording: String::new(),
        };
        let session = app
            .call(
                "POST",
                "api/v1/setup",
                json!({"username":"listener", "password":"private native desktop password"}),
            )
            .await;
        app.token = session["token"].as_str().unwrap().into();
        let libraries = app.call("GET", "api/v1/libraries", Value::Null).await;
        let jobs = app
            .call(
                "POST",
                &format!(
                    "api/v1/libraries/{}/scan",
                    libraries[0]["id"].as_str().unwrap()
                ),
                Value::Null,
            )
            .await;
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let job = app
                    .call(
                        "GET",
                        &format!("api/v1/jobs/{}", jobs[0]["id"].as_str().unwrap()),
                        Value::Null,
                    )
                    .await;
                if job["state"] == "complete" {
                    break;
                }
                assert_ne!(job["state"], "failed");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let recordings = app.call("GET", "api/v1/recordings", Value::Null).await;
        app.recording = recordings["items"][0]["id"].as_str().unwrap().into();
        app
    }
    pub fn paths(&self) -> Paths {
        paths(self.directory.path())
    }
    pub async fn request(&self, method: &str, path: &str, body: Value) -> Response {
        let mut request = self
            .client
            .request(
                method.parse::<Method>().unwrap(),
                self.root.join(path).unwrap(),
            )
            .header("cookie", &self.cookie)
            .header("origin", self.root.origin().ascii_serialization())
            .header("x-hito-request", "1")
            .json(&body);
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        request.send().await.unwrap()
    }
    pub async fn call(&self, method: &str, path: &str, body: Value) -> Value {
        let response = self.request(method, path, body).await;
        assert!(
            response.status().is_success(),
            "Request failed: {}",
            response.status()
        );
        response.json().await.unwrap()
    }
    pub async fn command(
        &self,
        lease: &Value,
        sequence: u64,
        revision: u64,
        action: Value,
    ) -> Value {
        self.call("POST", "api/v1/player/commands", json!({"id":Uuid::new_v4(), "lease":lease, "sequence":sequence, "revision":revision, "action":action})).await
    }
    pub async fn observe(&self, lease: &Value, kind: &str) -> Observation {
        let response = self
            .request(
                "POST",
                &format!("api/v1/player/{kind}/events"),
                json!({"lease":lease}),
            )
            .await;
        assert!(response.status().is_success());
        Observation {
            response,
            buffered: Vec::new(),
        }
    }
}
pub fn paths(directory: &std::path::Path) -> Paths {
    Paths {
        data: directory.join("data"),
        music: directory.join("music"),
        web: directory.join("web"),
    }
}
pub struct Observation {
    response: Response,
    buffered: Vec<u8>,
}
impl Observation {
    pub async fn next(&mut self) -> Option<Value> {
        loop {
            if let Some(end) = self.buffered.windows(2).position(|bytes| bytes == b"\n\n") {
                let bytes = self.buffered.drain(..end + 2).collect::<Vec<_>>();
                if let Some(data) = bytes.strip_prefix(b"data: ") {
                    return Some(serde_json::from_slice(data).unwrap());
                }
            } else {
                let chunk = self.response.chunk().await.unwrap()?;
                self.buffered.extend_from_slice(&chunk);
                assert!(self.buffered.len() <= 128 * 1024);
            }
        }
    }
    pub async fn until(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                let value = self.next().await.expect("Observation closed");
                if predicate(&value) {
                    break value;
                }
            }
        })
        .await
        .unwrap()
    }
}
