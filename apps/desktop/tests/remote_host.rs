use axum::{
    Json, Router,
    body::Bytes,
    extract::Request,
    http::{HeaderMap, StatusCode as AxumStatus, header},
    response::IntoResponse,
    routing::any,
};
use futures_util::future::BoxFuture;
use hito_desktop::{RemoteHost, current_server};
use hito_server::{HostPlace, HostPlaceCurrent};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::net::TcpListener;
use url::Url;

struct ChosenServer {
    origin: Url,
    changes: AtomicUsize,
}

impl HostPlace for ChosenServer {
    fn current(&self) -> HostPlaceCurrent {
        current_server(&self.origin)
    }

    fn change(&self) -> BoxFuture<'_, ()> {
        self.changes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {})
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
        let entry = host.entry_url();
        let bootstrap = client.get(entry.clone()).send().await.unwrap();
        assert_eq!(bootstrap.status(), StatusCode::SEE_OTHER);
        let cookie = bootstrap.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        Self {
            client,
            root: entry.join("/").unwrap(),
            cookie,
        }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!(
                "{}{}",
                self.root.as_str().trim_end_matches('/'),
                path
            ))
            .header("cookie", &self.cookie)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(self.root.join(path).unwrap())
            .header("cookie", &self.cookie)
            .header("origin", self.root.origin().ascii_serialization())
    }
}

async fn echo(headers: HeaderMap, request: Request) -> impl IntoResponse {
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    let body = axum::body::to_bytes(request.into_body(), 1 << 20)
        .await
        .unwrap_or_else(|_| Bytes::new());
    let seen = |name: &str| {
        headers
            .get(name)
            .map(|value| value.to_str().unwrap().to_owned())
    };
    (
        [
            (
                header::SET_COOKIE,
                "hito_session=server; Path=/; HttpOnly".to_owned(),
            ),
            (header::CONTENT_LANGUAGE, "en".to_owned()),
        ],
        Json(json!({
            "method": method,
            "uri": uri,
            "body": String::from_utf8_lossy(&body),
            "cookie": seen("cookie"),
            "origin": seen("origin"),
            "inertia_version": seen("x-inertia-version"),
            "hito_request": seen("x-hito-request"),
            "host": seen("host"),
        })),
    )
}

async fn server(hits: Arc<AtomicUsize>) -> Url {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = Url::parse(&format!("http://{address}/")).unwrap();
    let location = format!("{origin}library?after=sign-in");
    let router = Router::new()
        .route(
            "/moved",
            any(move || {
                let location = location.clone();
                async move { (AxumStatus::SEE_OTHER, [(header::LOCATION, location)]) }
            }),
        )
        .fallback(any(move |headers: HeaderMap, request: Request| {
            hits.fetch_add(1, Ordering::SeqCst);
            echo(headers, request)
        }));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    origin
}

fn bundle() -> tempfile::TempDir {
    let web = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(web.path().join("assets")).unwrap();
    std::fs::write(
        web.path().join("assets/app.js"),
        "export const bundled = true;\n",
    )
    .unwrap();
    web
}

async fn start(origin: &Url, web: &tempfile::TempDir) -> (RemoteHost, Arc<ChosenServer>) {
    let place = Arc::new(ChosenServer {
        origin: origin.clone(),
        changes: AtomicUsize::new(0),
    });
    let host = RemoteHost::start(origin.clone(), web.path(), place.clone())
        .await
        .unwrap();
    (host, place)
}

#[tokio::test]
async fn the_window_gets_the_apps_own_assets_and_everything_else_from_the_server() {
    let hits = Arc::new(AtomicUsize::new(0));
    let origin = server(hits.clone()).await;
    let web = bundle();
    let (host, _) = start(&origin, &web).await;
    let window = Window::open(&host).await;

    let asset = window.get("/assets/app.js").send().await.unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.text().await.unwrap(),
        "export const bundled = true;\n"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    let page = window
        .get("/library?sort=recent")
        .header("x-inertia", "true")
        .header("x-inertia-version", "bundled")
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(page.headers().get("set-cookie").is_none());
    assert_eq!(page.headers()["content-language"], "en");
    let seen: Value = page.json().await.unwrap();
    assert_eq!(seen["method"], "GET");
    assert_eq!(seen["uri"], "/library?sort=recent");
    assert_eq!(seen["cookie"], Value::Null);
    assert_eq!(seen["inertia_version"], Value::Null);
    assert_eq!(seen["host"], origin.authority());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_change_is_sent_with_its_body_and_without_the_windows_own_origin() {
    let origin = server(Arc::new(AtomicUsize::new(0))).await;
    let web = bundle();
    let (host, _) = start(&origin, &web).await;
    let window = Window::open(&host).await;
    let seen: Value = window
        .post("api/v1/playlists")
        .header("x-hito-request", "1")
        .json(&json!({"name": "Evening"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(seen["method"], "POST");
    assert_eq!(seen["uri"], "/api/v1/playlists");
    assert_eq!(seen["body"], r#"{"name":"Evening"}"#);
    assert_eq!(seen["origin"], Value::Null);
    assert_eq!(seen["hito_request"], "1");
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_path_that_looks_like_another_host_stays_on_the_chosen_server() {
    let hits = Arc::new(AtomicUsize::new(0));
    let origin = server(hits.clone()).await;
    let web = bundle();
    let (host, _) = start(&origin, &web).await;
    let window = Window::open(&host).await;
    let seen: Value = window
        .get("//elsewhere.example/steal")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(seen["uri"], "//elsewhere.example/steal");
    assert_eq!(seen["host"], origin.authority());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_redirect_on_the_server_stays_inside_the_window() {
    let origin = server(Arc::new(AtomicUsize::new(0))).await;
    let web = bundle();
    let (host, _) = start(&origin, &web).await;
    let window = Window::open(&host).await;
    let moved = window.get("/moved").send().await.unwrap();
    assert_eq!(moved.status(), StatusCode::SEE_OTHER);
    assert_eq!(moved.headers()["location"], "/library?after=sign-in");
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_place_row_names_the_server_and_change_reaches_the_app() {
    let hits = Arc::new(AtomicUsize::new(0));
    let origin = server(hits.clone()).await;
    let web = bundle();
    let (host, place) = start(&origin, &web).await;
    let window = Window::open(&host).await;
    let offer: Value = window
        .get("/api/v1/host/place")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        offer,
        json!({"changeable": true, "current": {"kind": "server", "origin": origin.origin().ascii_serialization()}})
    );
    let change = window
        .post("api/v1/host/place/change")
        .header("x-hito-request", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(change.status(), StatusCode::NO_CONTENT);
    assert_eq!(place.changes.load(Ordering::SeqCst), 1);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn nothing_reaches_the_server_without_the_windows_access() {
    let hits = Arc::new(AtomicUsize::new(0));
    let origin = server(hits.clone()).await;
    let web = bundle();
    let (host, _) = start(&origin, &web).await;
    let root = host.entry_url().join("/").unwrap();
    let denied = Client::new()
        .get(root.join("library").unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_server_that_stops_answering_is_reported_to_the_window() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    drop(listener);
    let web = bundle();
    let (host, _) = start(&origin, &web).await;
    let window = Window::open(&host).await;
    let answer = window.get("/library").send().await.unwrap();
    assert_eq!(answer.status(), StatusCode::BAD_GATEWAY);
    host.shutdown().await.unwrap();
}
