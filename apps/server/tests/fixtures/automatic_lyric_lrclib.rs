use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::Method,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

pub const TOKEN: &str = "owned-automatic-lrclib-service-token";
pub const TITLE: &str = "Owned automatic title";
pub const ARTIST: &str = "Owned automatic artist";

pub struct Server {
    pub url: String,
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Server {
    pub async fn start(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, receive) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = receive.await;
                })
                .await
                .unwrap();
        });
        Self {
            url,
            stop: Some(stop),
            task: Some(task),
        }
    }
    pub async fn finish(mut self) {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
#[derive(Clone)]
struct StateData(Arc<Mutex<Data>>);
struct Data {
    calls: Vec<&'static str>,
    records: Vec<Value>,
    record_ids: Vec<u64>,
}
pub fn record(id: u64) -> Value {
    json!({"id":id,"trackName":TITLE,"artistName":ARTIST,"albumName":"Owned album","duration":1.0,"instrumental":false,"plainLyrics":"Owned automatic plain text","syncedLyrics":null})
}
pub struct Upstream {
    pub server: Server,
    state: StateData,
}
impl Upstream {
    pub async fn start() -> Self {
        Self::with_records(vec![record(4242)]).await
    }
    pub async fn with_records(records: Vec<Value>) -> Self {
        let state = StateData(Arc::new(Mutex::new(Data {
            calls: Vec::new(),
            records,
            record_ids: Vec::new(),
        })));
        let server =
            Server::start(Router::new().fallback(exchange).with_state(state.clone())).await;
        Self { server, state }
    }
    pub fn assert_calls(&self, expected: &[&str]) {
        assert_eq!(self.state.0.lock().unwrap().calls, expected);
    }
    pub fn assert_record_ids(&self, expected: &[u64]) {
        assert_eq!(self.state.0.lock().unwrap().record_ids, expected);
    }
}
async fn exchange(State(state): State<StateData>, request: Request) -> Json<serde_json::Value> {
    assert_eq!(request.method(), Method::GET);
    assert!(request.headers().get("authorization").is_none());
    assert!(request.headers().get("cookie").is_none());
    assert!(request.headers().get("x-hito-plugin-protocol").is_none());
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    assert!(to_bytes(request.into_body(), 1).await.unwrap().is_empty());
    match path.as_str() {
        "/api/search" => {
            assert_eq!(
                url::form_urlencoded::parse(query.as_deref().unwrap().as_bytes())
                    .into_owned()
                    .collect::<Vec<_>>(),
                vec![("q".into(), format!("{ARTIST} {TITLE}"))]
            );
            let mut data = state.0.lock().unwrap();
            data.calls.push("search");
            Json(json!(data.records))
        }
        _ if path.starts_with("/api/get/") => {
            assert!(query.is_none());
            let id: u64 = path.strip_prefix("/api/get/").unwrap().parse().unwrap();
            assert_eq!(path, format!("/api/get/{id}"));
            let mut data = state.0.lock().unwrap();
            let record = data
                .records
                .iter()
                .find(|record| record["id"] == id)
                .unwrap()
                .clone();
            data.calls.push("record");
            data.record_ids.push(id);
            Json(record)
        }
        _ => panic!("Automatic lyrics reached an unexpected upstream route"),
    }
}
