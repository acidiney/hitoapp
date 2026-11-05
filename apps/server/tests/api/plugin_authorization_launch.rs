use super::provider_fixture::ProviderServer;
use super::support::{admin, call, services};
use axum::{Router, http::StatusCode};
use futures_util::future::BoxFuture;
use hito_server::{HostBrowser, HostOpen};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Recorder {
    opened: Mutex<Vec<String>>,
    fail: bool,
}

impl HostBrowser for Recorder {
    fn open(&self, url: url::Url) -> BoxFuture<'_, HostOpen> {
        Box::pin(async move {
            if self.fail {
                return HostOpen::Failed;
            }
            self.opened.lock().unwrap().push(url.into());
            HostOpen::Opened
        })
    }
}

struct Fixture {
    app: Router,
    recorder: Arc<Recorder>,
    token: String,
    installation: Value,
    _provider: ProviderServer,
    _directory: tempfile::TempDir,
}

async fn fixture(browser: Option<Arc<Recorder>>) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let services = services(directory.path(), &directory.path().join("music")).await;
    let recorder = browser.clone().unwrap_or_default();
    let config = hito_server::Config {
        origin: "https://hito.test".into(),
        ..Default::default()
    };
    let app = match browser {
        Some(browser) => {
            hito_server::router_with_hosts(services, config, None, None, Some(browser))
        }
        None => hito_server::router_with_config(services, config),
    };
    let token = admin(&app).await;
    let provider = ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.launch-fixture",
                "name": "Launch fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"],
                "authorization": {
                    "kind": "oauth2_pkce",
                    "client_id": "owned-test-client",
                    "authorization_endpoint": "https://accounts.example/authorize",
                    "token_endpoint": "https://accounts.example/token",
                    "scopes": ["playlist-read-private"]
                }
            }),
        )
        .await;
    let (_, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Launch fixture", "base_url": provider.url}),
        Some(&token),
    )
    .await;
    let (status, installation) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    Fixture {
        app,
        recorder,
        token,
        installation,
        _provider: provider,
        _directory: directory,
    }
}

impl Fixture {
    fn base(&self) -> String {
        format!(
            "/api/v1/plugins/{}/authorization",
            self.installation["id"].as_str().unwrap()
        )
    }

    async fn begin(&self) -> Value {
        let (status, attempt) = call(
            &self.app,
            "POST",
            &self.base(),
            json!({"revision": self.installation["revision"]}),
            Some(&self.token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        attempt
    }

    async fn launch(&self, attempt: &str, url: &str, token: &str) -> StatusCode {
        call(
            &self.app,
            "POST",
            &format!("{}/{attempt}/launch", self.base()),
            json!({"authorization_url": url}),
            Some(token),
        )
        .await
        .0
    }

    fn opened(&self) -> Vec<String> {
        self.recorder.opened.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn the_host_browser_receives_only_the_exact_url_of_a_live_attempt() {
    let fixture = fixture(Some(Arc::default())).await;
    let attempt = fixture.begin().await;
    let id = attempt["id"].as_str().unwrap();
    let url = attempt["authorization_url"].as_str().unwrap();
    assert_eq!(
        fixture.launch(id, url, &fixture.token).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(fixture.opened(), vec![url.to_string()]);
}

#[tokio::test]
async fn forged_and_foreign_urls_are_refused_without_reaching_the_browser() {
    let fixture = fixture(Some(Arc::default())).await;
    let attempt = fixture.begin().await;
    let other = fixture.begin().await;
    let id = attempt["id"].as_str().unwrap();
    let url = attempt["authorization_url"].as_str().unwrap();
    let refused = [
        "https://evil.example/authorize".to_string(),
        url.replace("accounts.example", "evil.example"),
        url.replace("https://", "http://"),
        url.replace("/authorize", "/other"),
        other["authorization_url"].as_str().unwrap().to_string(),
        "file:///etc/passwd".to_string(),
        String::new(),
    ];
    for candidate in refused {
        assert_eq!(
            fixture.launch(id, &candidate, &fixture.token).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{candidate}"
        );
    }
    assert_eq!(
        fixture.launch("no-such-attempt", url, &fixture.token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.launch(id, url, "invalid-token").await,
        StatusCode::UNAUTHORIZED
    );
    assert!(fixture.opened().is_empty());
}

#[tokio::test]
async fn another_session_or_account_cannot_launch_the_attempt() {
    let fixture = fixture(Some(Arc::default())).await;
    let attempt = fixture.begin().await;
    let id = attempt["id"].as_str().unwrap();
    let url = attempt["authorization_url"].as_str().unwrap();
    let (_, second) = call(
        &fixture.app,
        "POST",
        "/api/v1/login",
        json!({"username": "admin", "password": "administrator test password"}),
        None,
    )
    .await;
    assert_eq!(
        fixture
            .launch(id, url, second["token"].as_str().unwrap())
            .await,
        StatusCode::NOT_FOUND
    );
    assert!(fixture.opened().is_empty());
}

#[tokio::test]
async fn a_host_without_a_browser_launcher_reports_not_found() {
    let fixture = fixture(None).await;
    let attempt = fixture.begin().await;
    let status = fixture
        .launch(
            attempt["id"].as_str().unwrap(),
            attempt["authorization_url"].as_str().unwrap(),
            &fixture.token,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_launcher_failure_is_a_safe_error_and_the_attempt_stays_usable() {
    let fixture = fixture(Some(Arc::new(Recorder {
        fail: true,
        ..Default::default()
    })))
    .await;
    let attempt = fixture.begin().await;
    let id = attempt["id"].as_str().unwrap();
    let (status, body) = call(
        &fixture.app,
        "POST",
        &format!("{}/{id}/launch", fixture.base()),
        json!({"authorization_url": attempt["authorization_url"]}),
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(!body.to_string().contains("accounts.example"));
    let (status, pending) = call(
        &fixture.app,
        "GET",
        &format!("{}/{id}", fixture.base()),
        Value::Null,
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending["status"], "pending");
}
