use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone)]
pub enum Photo {
    Png(Vec<u8>),
    Jpeg(Vec<u8>),
    Html,
    Mislabelled(Vec<u8>),
    NotFound,
    Unavailable,
    Redirect(String),
}

#[derive(Default)]
struct State_ {
    entities: HashMap<String, Option<String>>,
    photos: HashMap<String, Photo>,
    requests: Vec<String>,
}

pub struct FakeArtistImages {
    pub endpoint: url::Url,
    state: Arc<Mutex<State_>>,
    server: JoinHandle<()>,
}

impl FakeArtistImages {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(State_::default()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let router = Router::new().fallback(respond).with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            endpoint,
            state,
            server,
        }
    }

    pub fn entity(&self, entity: &str, file: Option<&str>) {
        self.state
            .lock()
            .unwrap()
            .entities
            .insert(entity.into(), file.map(str::to_owned));
    }

    pub fn photo(&self, file: &str, photo: Photo) {
        self.state.lock().unwrap().photos.insert(file.into(), photo);
    }

    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().requests.clone()
    }
}

impl Drop for FakeArtistImages {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn reply(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap()
}

async fn respond(State(state): State<Arc<Mutex<State_>>>, request: Request) -> Response {
    let target = request
        .uri()
        .path_and_query()
        .map(ToString::to_string)
        .unwrap_or_default();
    let path = request.uri().path().to_owned();
    let mut state = state.lock().unwrap();
    state.requests.push(target);
    if let Some(rest) = path.strip_prefix("/wiki/Special:EntityData/") {
        let entity = rest.trim_end_matches(".json");
        return match state.entities.get(entity) {
            Some(file) => {
                let claims = file.as_ref().map(
                    |file| json!([{"rank": "normal", "mainsnak": {"datavalue": {"value": file}}}]),
                );
                reply(
                    StatusCode::OK,
                    "application/json",
                    json!({"entities": {entity: {"claims": claims.map(|claims| json!({"P18": claims})).unwrap_or(json!({}))}}})
                        .to_string()
                        .into_bytes(),
                )
            }
            None => reply(StatusCode::NOT_FOUND, "application/json", b"{}".to_vec()),
        };
    }
    if let Some(name) = path.strip_prefix("/wiki/Special:FilePath/") {
        let found = state.photos.contains_key(name);
        return if found {
            Response::builder()
                .status(StatusCode::FOUND)
                .header("location", format!("/thumb/{name}"))
                .body(Body::empty())
                .unwrap()
        } else {
            reply(StatusCode::NOT_FOUND, "text/plain", Vec::new())
        };
    }
    if let Some(name) = path.strip_prefix("/thumb/") {
        return match state.photos.get(name).cloned() {
            Some(Photo::Png(bytes)) => reply(StatusCode::OK, "image/png", bytes),
            Some(Photo::Jpeg(bytes)) => reply(StatusCode::OK, "image/jpeg", bytes),
            Some(Photo::Mislabelled(bytes)) => reply(StatusCode::OK, "image/png", bytes),
            Some(Photo::Html) => reply(StatusCode::OK, "text/html", b"<html></html>".to_vec()),
            Some(Photo::Unavailable) => {
                reply(StatusCode::SERVICE_UNAVAILABLE, "text/plain", Vec::new())
            }
            Some(Photo::Redirect(location)) => Response::builder()
                .status(StatusCode::FOUND)
                .header("location", location)
                .body(Body::empty())
                .unwrap(),
            Some(Photo::NotFound) | None => reply(StatusCode::NOT_FOUND, "text/plain", Vec::new()),
        };
    }
    reply(StatusCode::NOT_FOUND, "text/plain", Vec::new())
}
