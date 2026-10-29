use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use hito_plugin_listenbrainz::{Config, router};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;

const TOKEN: &str = "service-token-for-tests";
const CONTACT: &str = "operator@example.org";
const SEED: &str = "11111111-1111-4111-8111-111111111111";
const A: &str = "22222222-2222-4222-8222-222222222222";
const B: &str = "33333333-3333-4333-8333-333333333333";

#[derive(Clone)]
enum Reply {
    Json(Value),
    Status(StatusCode),
    Body(Vec<u8>),
    Typed(&'static str, String),
    Limited(u64),
}

#[derive(Clone)]
struct Fake {
    reply: Arc<Mutex<Reply>>,
    seen: Arc<Mutex<Vec<(String, String, String)>>>,
}

async fn labs(State(fake): State<Fake>, request: Request) -> Response {
    let agent = request
        .headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    fake.seen.lock().unwrap().push((
        request.method().to_string(),
        request.uri().to_string(),
        agent,
    ));
    match fake.reply.lock().unwrap().clone() {
        Reply::Json(value) => Json(value).into_response(),
        Reply::Status(status) => status.into_response(),
        Reply::Body(bytes) => bytes.into_response(),
        Reply::Typed(kind, body) => ([("content-type", kind)], body).into_response(),
        Reply::Limited(seconds) => (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", seconds.to_string())],
        )
            .into_response(),
    }
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn start(reply: Reply) -> (String, Fake) {
    let fake = Fake {
        reply: Arc::new(Mutex::new(reply)),
        seen: Arc::default(),
    };
    let labs_url = serve(Router::new().fallback(any(labs)).with_state(fake.clone())).await;
    let config = Config::new(CONTACT, TOKEN)
        .unwrap()
        .with_endpoint(Url::parse(&labs_url).unwrap())
        .unwrap()
        .with_min_interval(Duration::ZERO);
    (serve(router(config).unwrap()).await, fake)
}

async fn post(plugin: &str, body: Value) -> (StatusCode, Value) {
    let response =
        authorized(reqwest::Client::new().post(format!("{plugin}v1/catalog/similarity/artists")))
            .json(&body)
            .send()
            .await
            .unwrap();
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn authorized(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request
        .bearer_auth(TOKEN)
        .header("x-hito-plugin-protocol", "1")
}

fn entry(id: &str, name: &str, score: Value) -> Value {
    json!({"artist_mbid": id, "name": name, "comment": "", "type": "Group", "gender": null,
        "reference_mbid": SEED, "score": score})
}

#[tokio::test]
async fn the_plugin_declares_only_the_similarity_contract() {
    let (plugin, fake) = start(Reply::Status(StatusCode::INTERNAL_SERVER_ERROR)).await;
    let manifest: Value = authorized(reqwest::Client::new().get(format!("{plugin}v1/manifest")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["capabilities"], json!(["catalog"]));
    let descriptor: Value = authorized(
        reqwest::Client::new().get(format!("{plugin}v1/catalog/similarity/capabilities")),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(
        descriptor,
        json!({"similarity_version": 1, "operations": ["artist_similarity"], "artist_identity": "musicbrainz_artist"})
    );
    let profile =
        authorized(reqwest::Client::new().get(format!("{plugin}v1/catalog/capabilities")))
            .send()
            .await
            .unwrap();
    assert_eq!(profile.status(), 404);
    assert!(fake.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn similar_artists_are_mapped_bounded_and_sent_only_an_identity() {
    let (plugin, fake) = start(Reply::Json(json!([
        entry(A, "Segunda", json!(800)),
        entry(SEED, "A própria", json!(700)),
        entry("not-an-mbid", "Inválida", json!(600)),
        entry(A, "Repetida", json!(500)),
        entry(B, "Terceira", json!(null)),
        entry("44444444-4444-4444-8444-444444444444", "", json!(300)),
        entry("55555555-5555-4555-8555-555555555555", "Quinta", json!(200)),
    ])))
    .await;
    let (status, page) = post(&plugin, json!({"artist_id": SEED, "limit": 2})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page,
        json!({"artist_id": SEED, "items": [
            {"artist": {"id": A, "name": "Segunda"}, "score": 800.0},
            {"artist": {"id": B, "name": "Terceira"}, "score": null}
        ]})
    );
    let seen = fake.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let (method, uri, agent) = &seen[0];
    assert_eq!(method, "GET");
    assert!(uri.starts_with("/similar-artists/json?"));
    let query = Url::parse(&format!("http://x{uri}")).unwrap();
    let mut pairs: Vec<(String, String)> = query
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].0, "algorithm");
    assert!(pairs[0].1.starts_with("session_based_days_"));
    assert_eq!(pairs[1], ("artist_mbids".into(), SEED.into()));
    assert!(
        agent.starts_with("Hito/") && agent.contains("ListenBrainz") && agent.contains(CONTACT)
    );
}

#[tokio::test]
async fn invalid_requests_and_failing_or_hostile_upstreams_are_contained() {
    let (plugin, fake) = start(Reply::Json(json!([]))).await;
    for body in [
        json!({"artist_id": "not-an-mbid", "limit": 5}),
        json!({"artist_id": SEED, "limit": 0}),
        json!({"artist_id": SEED, "limit": 5, "listens": [1]}),
    ] {
        assert_eq!(post(&plugin, body).await.0, StatusCode::BAD_REQUEST);
    }
    assert!(fake.seen.lock().unwrap().is_empty());
    assert_eq!(
        post(&plugin, json!({"artist_id": SEED, "limit": 5})).await,
        (StatusCode::OK, json!({"artist_id": SEED, "items": []}))
    );
    for reply in [
        Reply::Status(StatusCode::TOO_MANY_REQUESTS),
        Reply::Status(StatusCode::INTERNAL_SERVER_ERROR),
        Reply::Body(b"<html>not json</html>".to_vec()),
        Reply::Json(json!({"not": "a list"})),
        Reply::Body(vec![b' '; 600_000]),
    ] {
        *fake.reply.lock().unwrap() = reply;
        let (status, body) = post(&plugin, json!({"artist_id": SEED, "limit": 5})).await;
        assert!(
            status == StatusCode::BAD_GATEWAY || status == StatusCode::SERVICE_UNAVAILABLE,
            "{status}"
        );
        assert!(body["error"].is_string());
    }
}

#[tokio::test]
async fn every_route_requires_the_service_bearer_and_protocol_header() {
    let (plugin, fake) = start(Reply::Json(json!([]))).await;
    let client = reqwest::Client::new();
    let routes = [
        ("GET", "v1/manifest"),
        ("GET", "v1/catalog/similarity/capabilities"),
        ("POST", "v1/catalog/similarity/artists"),
    ];
    for (method, route) in routes {
        let build = |token: Option<&str>, protocol: bool| {
            let mut request = if method == "GET" {
                client.get(format!("{plugin}{route}"))
            } else {
                client
                    .post(format!("{plugin}{route}"))
                    .json(&json!({"artist_id": SEED, "limit": 5}))
            };
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            if protocol {
                request = request.header("x-hito-plugin-protocol", "1");
            }
            request
        };
        for (token, protocol) in [
            (None, true),
            (Some("wrong-token"), true),
            (Some(TOKEN), false),
        ] {
            let status = build(token, protocol).send().await.unwrap().status();
            assert!(
                status == 401 || status == 400,
                "{route} {token:?} {protocol} gave {status}"
            );
        }
        assert_eq!(build(Some(TOKEN), true).send().await.unwrap().status(), 200);
    }
    assert_eq!(
        fake.seen.lock().unwrap().len(),
        1,
        "only the authorized search reached the service"
    );
}

#[test]
fn the_operator_contact_and_service_token_are_required_and_header_safe() {
    for contact in [
        "",
        "no contact here",
        "bad\r\nx-injected: 1@example.org",
        "(parens)@example.org",
        &format!("{}@example.org", "a".repeat(200)),
        "plain-text",
    ] {
        assert!(Config::new(contact, TOKEN).is_err(), "{contact:?}");
    }
    for contact in ["ops@example.org", "https://example.org/contact"] {
        assert!(Config::new(contact, TOKEN).is_ok(), "{contact:?}");
    }
    for token in ["", "has space", "tab\there", &"t".repeat(4097)] {
        assert!(Config::new(CONTACT, token).is_err(), "{token:?}");
    }
}

#[tokio::test]
async fn an_unknown_artist_is_empty_and_a_non_json_answer_is_invalid() {
    let (plugin, fake) = start(Reply::Status(StatusCode::NOT_FOUND)).await;
    assert_eq!(
        post(&plugin, json!({"artist_id": SEED, "limit": 5})).await,
        (StatusCode::OK, json!({"artist_id": SEED, "items": []}))
    );
    *fake.reply.lock().unwrap() = Reply::Typed("text/plain", "[]".into());
    let (status, body) = post(&plugin, json!({"artist_id": SEED, "limit": 5})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body["error"].is_string());
    *fake.reply.lock().unwrap() = Reply::Typed("application/json; charset=utf-8", "[]".into());
    assert_eq!(
        post(&plugin, json!({"artist_id": SEED, "limit": 5}))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_rate_limit_answer_backs_the_plugin_off_without_asking_again() {
    let (plugin, fake) = start(Reply::Limited(30)).await;
    let (status, _) = post(&plugin, json!({"artist_id": SEED, "limit": 5})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    *fake.reply.lock().unwrap() = Reply::Json(json!([]));
    let (status, _) = post(&plugin, json!({"artist_id": SEED, "limit": 5})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        fake.seen.lock().unwrap().len(),
        1,
        "no request during the back-off"
    );
}

#[test]
fn the_service_refuses_to_start_without_a_contact_or_service_token() {
    let binary = env!("CARGO_BIN_EXE_hito-plugin-listenbrainz");
    for arguments in [
        vec![],
        vec!["--listen", "127.0.0.1:0"],
        vec!["--listen", "127.0.0.1:0", "--contact", "ops@example.org"],
    ] {
        let mut child = std::process::Command::new(binary)
            .args(&arguments)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if started.elapsed() > Duration::from_secs(3) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            status.is_some_and(|status| !status.success()),
            "{arguments:?} must exit with a failure"
        );
    }
}
