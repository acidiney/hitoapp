use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use hito_plugin_listenbrainz::{Config, router};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use url::Url;

const SEED: &str = "11111111-1111-4111-8111-111111111111";
const A: &str = "22222222-2222-4222-8222-222222222222";
const B: &str = "33333333-3333-4333-8333-333333333333";

#[derive(Clone)]
enum Reply {
    Json(Value),
    Status(StatusCode),
    Body(Vec<u8>),
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
    let config = Config::with_endpoint(Url::parse(&labs_url).unwrap()).unwrap();
    (serve(router(config).unwrap()).await, fake)
}

async fn post(plugin: &str, body: Value) -> (StatusCode, Value) {
    let response = reqwest::Client::new()
        .post(format!("{plugin}v1/catalog/similarity/artists"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn entry(id: &str, name: &str, score: Value) -> Value {
    json!({"artist_mbid": id, "name": name, "comment": "", "type": "Group", "gender": null,
        "reference_mbid": SEED, "score": score})
}

#[tokio::test]
async fn the_plugin_declares_only_the_similarity_contract() {
    let (plugin, fake) = start(Reply::Status(StatusCode::INTERNAL_SERVER_ERROR)).await;
    let manifest: Value = reqwest::get(format!("{plugin}v1/manifest"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["capabilities"], json!(["catalog"]));
    let descriptor: Value = reqwest::get(format!("{plugin}v1/catalog/similarity/capabilities"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        descriptor,
        json!({"similarity_version": 1, "operations": ["artist_similarity"], "artist_identity": "musicbrainz_artist"})
    );
    let profile = reqwest::get(format!("{plugin}v1/catalog/capabilities"))
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
    assert!(agent.starts_with("Hito/") && agent.contains("ListenBrainz"));
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
