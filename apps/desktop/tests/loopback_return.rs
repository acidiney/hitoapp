use axum::{Json, Router, extract::State, http::StatusCode as Status, routing::get, routing::post};
use base64::Engine;
use hito_desktop::{LocalHost, Paths, ReturnPort, SystemBrowser};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::{
    net::{SocketAddr, TcpListener},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Issued {
    challenges: Mutex<HashMap<String, String>>,
}

type Fixture = (Arc<Mutex<Vec<String>>>, Arc<Issued>);

struct Provider {
    port: u16,
    tokens: Arc<Mutex<Vec<String>>>,
    issued: Arc<Issued>,
}

impl Provider {
    fn issue(&self, code: &str, authorization_url: &str) {
        self.issued
            .challenges
            .lock()
            .unwrap()
            .insert(code.into(), pair(authorization_url, "code_challenge"));
    }

    async fn redeem(&self, form: &str) -> reqwest::StatusCode {
        Client::new()
            .post(format!("http://127.0.0.1:{}/token", self.port))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form.to_owned())
            .send()
            .await
            .unwrap()
            .status()
    }
}

async fn provider() -> Provider {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let tokens = Arc::new(Mutex::new(Vec::new()));
    let issued = Arc::new(Issued::default());
    let shared = (tokens.clone(), issued.clone());
    let app = Router::new()
        .route(
            "/v1/manifest",
            get(move || async move {
                Json(json!({
                    "id": "org.hito.desktop-return-fixture",
                    "name": "Return fixture",
                    "protocol_version": 1,
                    "capabilities": ["playlists"],
                    "authorization": {
                        "kind": "oauth2_pkce",
                        "client_id": "return-client",
                        "authorization_endpoint": format!("http://127.0.0.1:{port}/authorize"),
                        "token_endpoint": format!("http://127.0.0.1:{port}/token"),
                        "scopes": ["playlist-read-private"]
                    }
                }))
            }),
        )
        .route(
            "/token",
            post(
                |State((tokens, issued)): State<Fixture>, body: String| async move {
                    tokens.lock().unwrap().push(body.clone());
                    let form: HashMap<String, String> =
                        url::form_urlencoded::parse(body.as_bytes())
                            .into_owned()
                            .collect();
                    let expected = form
                        .get("code")
                        .and_then(|code| issued.challenges.lock().unwrap().get(code).cloned());
                    let proven = form.get("code_verifier").is_some_and(|verifier| {
                        Some(
                            base64::engine::general_purpose::URL_SAFE_NO_PAD
                                .encode(Sha256::digest(verifier.as_bytes())),
                        ) == expected
                    });
                    if !proven {
                        return (Status::BAD_REQUEST, Json(json!({"error": "invalid_grant"})));
                    }
                    (
                        Status::OK,
                        Json(json!({
                            "access_token": "owned-desktop-access",
                            "token_type": "Bearer",
                            "expires_in": 3600,
                            "refresh_token": "owned-desktop-refresh"
                        })),
                    )
                },
            ),
        )
        .with_state(shared);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Provider {
        port,
        tokens,
        issued,
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct Desktop {
    host: LocalHost,
    client: Client,
    root: url::Url,
    cookie: String,
    token: String,
    _directory: tempfile::TempDir,
}

impl Desktop {
    async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .client
            .request(method.parse().unwrap(), self.root.join(path).unwrap())
            .header("cookie", &self.cookie)
            .header("x-hito-request", "1")
            .header("origin", self.root.origin().ascii_serialization())
            .header("authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .await
            .unwrap();
        (
            response.status(),
            response.json().await.unwrap_or(Value::Null),
        )
    }
}

async fn desktop(port: ReturnPort) -> Desktop {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let host = LocalHost::start_with_folders(
        Paths {
            data: directory.path().join("data"),
            music,
            web,
        },
        None,
        None,
        Some(Arc::new(SystemBrowser::new(|_| true))),
        port,
    )
    .await
    .unwrap();
    let entry = host.entry_url();
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let cookie = client.get(entry.clone()).send().await.unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut desktop = Desktop {
        host,
        client,
        root: entry.join("/").unwrap(),
        cookie,
        token: String::new(),
        _directory: directory,
    };
    let (status, body) = desktop
        .call(
            "POST",
            "api/v1/setup",
            json!({"username": "owner", "password": "desktop return test password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    desktop.token = body["token"].as_str().unwrap().to_owned();
    desktop
}

async fn installation(desktop: &Desktop, provider: &Provider) -> Value {
    let (status, service) = desktop
        .call(
            "POST",
            "api/v1/plugins/services",
            json!({"name": "Return fixture", "base_url": format!("http://127.0.0.1:{}", provider.port)}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{service}");
    let (status, installation) = desktop
        .call(
            "POST",
            "api/v1/plugins",
            json!({"service_id": service["id"], "service_revision": service["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{installation}");
    installation
}

async fn begin(desktop: &Desktop, installation: &Value) -> (StatusCode, Value) {
    desktop
        .call(
            "POST",
            &format!(
                "api/v1/plugins/{}/authorization",
                installation["id"].as_str().unwrap()
            ),
            json!({"revision": installation["revision"]}),
        )
        .await
}

fn pair(url: &str, name: &str) -> String {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == name)
        .unwrap()
        .1
        .into_owned()
}

async fn refused(port: u16) -> bool {
    tokio::net::TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .is_err()
}

#[test]
fn the_return_port_setting_defaults_to_the_documented_port_and_rejects_everything_else() {
    assert_eq!(ReturnPort::from_setting(None).unwrap().number(), 47321);
    assert_eq!(ReturnPort::default().number(), 47321);
    assert_eq!(
        ReturnPort::from_setting(Some(" 47400 ")).unwrap().number(),
        47400
    );
    for value in ["", "abc", "80", "1023", "65536", "-1", "47321.5", "0"] {
        assert!(ReturnPort::from_setting(Some(value)).is_err(), "{value}");
    }
}

#[tokio::test]
async fn the_system_browser_completes_the_sign_in_on_the_fixed_loopback_address_without_the_window_cookie()
 {
    let provider = provider().await;
    let desktop = desktop(ReturnPort::from_setting(Some(&free_port().to_string())).unwrap()).await;
    let installation = installation(&desktop, &provider).await;
    let (status, started) = begin(&desktop, &installation).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let authorization = started["authorization_url"].as_str().unwrap();
    let redirect = pair(authorization, "redirect_uri");
    let loopback: url::Url = redirect.parse().unwrap();
    assert_eq!(loopback.host_str(), Some("127.0.0.1"));
    assert_eq!(loopback.path(), "/api/v1/plugin-authorization/callback");
    assert_ne!(
        loopback.port(),
        desktop.root.port(),
        "the return address is not the private window backend"
    );
    let state = pair(authorization, "state");
    provider.issue("owned-code", authorization);
    let browser = Client::new();
    let page = browser
        .get(format!("{redirect}?code=owned-code&state={state}"))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()["referrer-policy"], "no-referrer");
    assert_eq!(page.headers()["cache-control"], "no-store");
    assert!(page.headers().get("set-cookie").is_none());
    let body = page.text().await.unwrap();
    assert!(body.contains("Provider authorized"));
    assert!(!body.contains(&state) && !body.contains("owned-code"));
    let exchanges = provider.tokens.lock().unwrap().clone();
    assert_eq!(exchanges.len(), 1);
    assert!(exchanges[0].contains(&format!(
        "redirect_uri={}",
        url::form_urlencoded::byte_serialize(redirect.as_bytes()).collect::<String>()
    )));
    let (_, connection) = desktop
        .call(
            "GET",
            &format!(
                "api/v1/plugins/{}/connection",
                installation["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(connection["status"], "connected");
    let replay = browser
        .get(format!("{redirect}?code=owned-code&state={state}"))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::CONFLICT);
    assert_eq!(provider.tokens.lock().unwrap().len(), 1);
    let private = browser
        .get(
            desktop
                .root
                .join("api/v1/plugin-authorization/callback?state=x")
                .unwrap(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(private.status(), StatusCode::FORBIDDEN);
    println!("fixed loopback sign-in completed at {redirect}");
    desktop.host.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_loopback_listener_serves_only_the_callback_and_guessed_states_fail() {
    let provider = provider().await;
    let desktop = desktop(ReturnPort::from_setting(Some(&free_port().to_string())).unwrap()).await;
    let installation = installation(&desktop, &provider).await;
    let (_, started) = begin(&desktop, &installation).await;
    let redirect = pair(
        started["authorization_url"].as_str().unwrap(),
        "redirect_uri",
    );
    let base = url::Url::parse(&redirect).unwrap();
    let browser = Client::new();
    for path in [
        "/",
        "/library",
        "/settings",
        "/api/v1/plugins",
        "/__desktop/start",
    ] {
        let response = browser.get(base.join(path).unwrap()).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert!(response.headers().get("set-cookie").is_none());
    }
    let guessed = browser
        .get(format!(
            "{redirect}?code=owned-code&state=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(guessed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(guessed.headers()["referrer-policy"], "no-referrer");
    assert_eq!(guessed.headers()["cache-control"], "no-store");
    assert!(provider.tokens.lock().unwrap().is_empty());
    desktop.host.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_loopback_port_is_closed_until_a_sign_in_begins_and_after_shutdown() {
    let provider = provider().await;
    let port = free_port();
    let desktop = desktop(ReturnPort::from_setting(Some(&port.to_string())).unwrap()).await;
    assert!(refused(port).await);
    let installation = installation(&desktop, &provider).await;
    let (_, first) = begin(&desktop, &installation).await;
    assert!(!refused(port).await);
    let (_, second) = begin(&desktop, &installation).await;
    assert_eq!(
        pair(first["authorization_url"].as_str().unwrap(), "redirect_uri"),
        pair(
            second["authorization_url"].as_str().unwrap(),
            "redirect_uri"
        )
    );
    assert_eq!(
        pair(first["authorization_url"].as_str().unwrap(), "redirect_uri"),
        format!("http://127.0.0.1:{port}/api/v1/plugin-authorization/callback")
    );
    desktop.host.shutdown().await.unwrap();
    assert!(refused(port).await);
}

#[tokio::test]
async fn a_port_in_use_fails_connect_with_a_plain_message_and_never_picks_another_port() {
    let provider = provider().await;
    let squatter = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = squatter.local_addr().unwrap().port();
    let desktop = desktop(ReturnPort::from_setting(Some(&port.to_string())).unwrap()).await;
    let installation = installation(&desktop, &provider).await;
    let (status, body) = begin(&desktop, &installation).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let message = body["error"].as_str().unwrap();
    assert!(
        message.contains(&format!("Port {port} is in use")),
        "{message}"
    );
    assert!(message.contains("HITO_DESKTOP_OAUTH_PORT"), "{message}");
    drop(squatter);
    let (status, started) = begin(&desktop, &installation).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    assert_eq!(
        pair(
            started["authorization_url"].as_str().unwrap(),
            "redirect_uri"
        ),
        format!("http://127.0.0.1:{port}/api/v1/plugin-authorization/callback")
    );
    desktop.host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_cross_site_or_misnamed_request_is_refused_and_a_captured_code_is_useless_without_the_verifier()
 {
    let provider = provider().await;
    let port = free_port();
    let desktop = desktop(ReturnPort::from_setting(Some(&port.to_string())).unwrap()).await;
    let installation = installation(&desktop, &provider).await;
    let (_, started) = begin(&desktop, &installation).await;
    let authorization = started["authorization_url"].as_str().unwrap();
    let state = pair(authorization, "state");
    provider.issue("captured-code", authorization);
    let target = format!(
        "http://127.0.0.1:{port}/api/v1/plugin-authorization/callback?code=captured-code&state={state}"
    );
    let browser = Client::new();
    let cross_site = browser
        .get(&target)
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);
    assert_eq!(cross_site.headers()["referrer-policy"], "no-referrer");
    for host in ["localhost", "evil.example", "127.0.0.1", "[::1]:1"] {
        let response = browser
            .get(&target)
            .header("host", host)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{host}");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    }
    assert!(provider.tokens.lock().unwrap().is_empty());

    let attacker_without_verifier = format!(
        "grant_type=authorization_code&code=captured-code&client_id=return-client&redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Fapi%2Fv1%2Fplugin-authorization%2Fcallback"
    );
    assert_eq!(
        provider.redeem(&attacker_without_verifier).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        provider
            .redeem(&format!(
                "{attacker_without_verifier}&code_verifier=guessed-verifier"
            ))
            .await,
        StatusCode::BAD_REQUEST
    );

    let (_, connection) = desktop
        .call(
            "GET",
            &format!(
                "api/v1/plugins/{}/connection",
                installation["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(connection["status"], "not_connected");
    let honest = browser.get(&target).send().await.unwrap();
    assert_eq!(honest.status(), StatusCode::OK);
    desktop.host.shutdown().await.unwrap();
}
