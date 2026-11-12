use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::Services;
#[path = "../fixtures/download_host.rs"]
pub mod host;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::PathBuf;
use tower::ServiceExt;

pub const ROOT: &str = "/api/v1/catalog/snapshots";
pub const PASSWORD: &str = "snapshot HTTP test password";

pub struct Fixture {
    pub app: Router,
    pub services: Services,
    pub token: String,
    pub account: String,
    pub library: String,
    pub music: PathBuf,
    pub directory: tempfile::TempDir,
}

impl Fixture {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let services = host::open(directory.path(), "snapshot-http-host").await;
        let app = hito_server::router(services.clone());
        let setup = request(
            &app,
            "POST",
            "/api/v1/setup",
            json!({"username":"admin", "password":PASSWORD}),
            None,
        )
        .await;
        assert_eq!(setup.status, StatusCode::CREATED);
        let token = setup.json()["token"].as_str().unwrap().to_owned();
        let account = request(&app, "GET", "/api/v1/me", Value::Null, Some(&token))
            .await
            .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let library = request(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token))
            .await
            .json()[0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            app,
            services,
            token,
            account,
            library,
            music,
            directory,
        }
    }

    pub async fn call(&self, method: &str, path: &str, body: Value) -> Reply {
        request(&self.app, method, path, body, Some(&self.token)).await
    }

    pub async fn begin(&self) -> Value {
        let reply = self
            .call("POST", ROOT, json!({"library_id":self.library}))
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
        reply.private();
        reply.json()
    }

    pub async fn scan(&self) {
        let reply = self
            .call(
                "POST",
                &format!("/api/v1/libraries/{}/scan", self.library),
                Value::Null,
            )
            .await;
        assert_eq!(reply.status, StatusCode::ACCEPTED);
        self.services.storage.run_next_job().await.unwrap();
        let job = reply.json()[0]["id"].as_str().unwrap().to_owned();
        tokio::time::timeout(std::time::Duration::from_secs(4), async {
            loop {
                let reply = self
                    .call("GET", &format!("/api/v1/jobs/{job}"), Value::Null)
                    .await;
                assert_eq!(reply.status, StatusCode::OK);
                match reply.json()["state"].as_str().unwrap() {
                    "complete" => break,
                    "queued" | "running" => tokio::task::yield_now().await,
                    other => panic!("The owned scan must complete, received {other}"),
                }
            }
        })
        .await
        .expect("The owned scan must finish within its deadline");
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap()
    }
    pub fn private(&self) {
        assert_eq!(self.headers["cache-control"], "private, no-store");
        assert_eq!(self.headers["x-content-type-options"], "nosniff");
        uuid::Uuid::parse_str(self.headers["x-request-id"].to_str().unwrap()).unwrap();
    }
}

pub async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-hito-request", "1");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    raw(app, builder.body(Body::from(body.to_string())).unwrap()).await
}

pub async fn raw(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
    }
}

pub fn tag_wav(path: &std::path::Path, title: &str) {
    use lofty::{
        prelude::{Accessor, TagExt},
        tag::{Tag, TagType},
    };
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Owned performer".into());
    tag.set_album("Owned release".into());
    tag.save_to_path(path, Default::default()).unwrap();
}

pub struct Member {
    pub id: String,
    pub token: String,
}

impl Fixture {
    pub async fn member(&self, username: &str) -> Member {
        let created = self
            .call(
                "POST",
                "/api/v1/users",
                json!({"username":username,"password":PASSWORD}),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED);
        let id = created.json()["id"].as_str().unwrap().to_owned();
        let login = request(
            &self.app,
            "POST",
            "/api/v1/login",
            json!({"username":username,"password":PASSWORD}),
            None,
        )
        .await;
        assert_eq!(login.status, StatusCode::OK);
        Member {
            id,
            token: login.json()["token"].as_str().unwrap().to_owned(),
        }
    }

    pub async fn access(&self, member: &str, granted: bool) {
        assert_eq!(
            self.call(
                if granted { "PUT" } else { "DELETE" },
                &format!("/api/v1/libraries/{}/members/{member}", self.library),
                Value::Null
            )
            .await
            .status,
            StatusCode::NO_CONTENT
        );
    }
}
