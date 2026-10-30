use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, Uri},
    response::Response,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone)]
pub enum Reply {
    /// Recordings that carry the code, each with its credited artist identifiers.
    Recordings(Vec<(String, Vec<String>)>),
    /// Artists related to a provider URL.
    Artists(Vec<String>),
    /// Artists found by a name search, each with its identifier and name.
    Candidates(Vec<(String, String)>),
    /// A search page with a stated total that may exceed the page.
    CandidatesOf(Vec<(String, String)>, u32),
    /// The first page of an artist's recordings: the total MusicBrainz states, then each title
    /// with its length in milliseconds.
    Discography(u32, Vec<(String, Option<u64>)>),
    /// One browse page of release groups, each with its identifier and primary type.
    ReleaseGroups(Vec<(String, Option<String>)>),
    NotFound,
    Unavailable,
}

#[derive(Default)]
struct State_ {
    replies: HashMap<String, Reply>,
    requests: Vec<String>,
    delay: Duration,
}

/// An owned loopback stand-in for the MusicBrainz web service.
pub struct FakeMusicBrainz {
    pub endpoint: url::Url,
    state: Arc<Mutex<State_>>,
    server: JoinHandle<()>,
}

impl FakeMusicBrainz {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(State_::default()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/ws/2/", listener.local_addr().unwrap())
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

    pub fn isrc(&self, isrc: &str, reply: Reply) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("isrc/{isrc}"), reply);
    }

    pub fn url(&self, resource: &str, reply: Reply) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("url/{resource}"), reply);
    }

    /// Answers an artist search for exactly this name.
    pub fn search(&self, name: &str, reply: Reply) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("search/{name}"), reply);
    }

    /// Answers an artist search whose total exceeds the returned page.
    pub fn search_total(&self, name: &str, found: Vec<(String, String)>, total: u32) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("search/{name}"), Reply::CandidatesOf(found, total));
    }

    /// Answers the recordings browse of one artist.
    pub fn recordings_of(&self, artist: &str, reply: Reply) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("recordings/{artist}"), reply);
    }

    /// Answers the release-group browse of one artist, for one primary type or for all of them.
    pub fn release_groups_of(&self, artist: &str, release_type: Option<&str>, reply: Reply) {
        self.state.lock().unwrap().replies.insert(
            format!("release-groups/{artist}/{}", release_type.unwrap_or("all")),
            reply,
        );
    }

    pub fn delay_replies(&self, delay: Duration) {
        self.state.lock().unwrap().delay = delay;
    }

    /// Request targets in arrival order, with the query string.
    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().requests.clone()
    }
}

impl Drop for FakeMusicBrainz {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn artist(id: &str) -> Value {
    json!({"name": "credited", "joinphrase": "", "artist": {"id": id, "name": "credited"}})
}

fn key(uri: &Uri) -> Option<String> {
    let path = uri.path().strip_prefix("/ws/2/")?;
    if let Some(isrc) = path.strip_prefix("isrc/") {
        return Some(format!("isrc/{isrc}"));
    }
    let parameter = |name: &str| -> Option<String> {
        let prefix = format!("{name}=");
        let encoded = uri
            .query()?
            .split('&')
            .find_map(|pair| pair.strip_prefix(prefix.as_str()))?;
        Some(
            url::form_urlencoded::parse(format!("r={encoded}").as_bytes())
                .next()?
                .1
                .into_owned(),
        )
    };
    if path == "artist" {
        let query = parameter("query")?;
        let name = query.strip_prefix('"')?.strip_suffix('"')?;
        return Some(format!("search/{name}"));
    }
    if path == "release-group" {
        return Some(format!(
            "release-groups/{}/{}",
            parameter("artist")?,
            parameter("type").unwrap_or_else(|| "all".into())
        ));
    }
    if path == "recording" {
        return Some(format!("recordings/{}", parameter("artist")?));
    }
    if path == "url" {
        let resource = uri
            .query()?
            .split('&')
            .find_map(|pair| pair.strip_prefix("resource="))?;
        let decoded = url::form_urlencoded::parse(format!("r={resource}").as_bytes())
            .next()?
            .1
            .into_owned();
        return Some(format!("url/{decoded}"));
    }
    None
}

async fn respond(State(state): State<Arc<Mutex<State_>>>, request: Request) -> Response {
    let (reply, delay) = {
        let mut state = state.lock().unwrap();
        state.requests.push(request.uri().to_string());
        let reply = key(request.uri()).and_then(|key| state.replies.get(&key).cloned());
        (reply, state.delay)
    };
    tokio::time::sleep(delay).await;
    let (status, body) = match (key(request.uri()), reply) {
        (_, Some(Reply::Recordings(recordings))) => {
            let isrc = request.uri().path().rsplit('/').next().unwrap_or_default();
            let recordings: Vec<Value> = recordings
                .iter()
                .map(|(id, artists)| {
                    json!({"id": id, "title": "Owned recording", "artist-credit":
                        artists.iter().map(|id| artist(id)).collect::<Vec<_>>()})
                })
                .collect();
            (
                StatusCode::OK,
                json!({"isrc": isrc, "recordings": recordings}),
            )
        }
        (Some(key), Some(Reply::Artists(artists))) => {
            let relations: Vec<Value> = artists
                .iter()
                .map(|id| json!({"type": "streaming", "target-type": "artist", "artist": {"id": id, "name": "credited"}}))
                .collect();
            (
                StatusCode::OK,
                json!({"id": "0f700000-0000-4000-8000-0000000000ff",
                    "resource": key.trim_start_matches("url/"), "relations": relations}),
            )
        }
        (_, Some(Reply::CandidatesOf(artists, total))) => {
            let found: Vec<Value> = artists
                .iter()
                .map(|(id, name)| json!({"id": id, "name": name, "type": "Person", "disambiguation": ""}))
                .collect();
            (
                StatusCode::OK,
                json!({"count": total, "offset": 0, "artists": found}),
            )
        }
        (_, Some(Reply::Candidates(artists))) => {
            let found: Vec<Value> = artists
                .iter()
                .map(|(id, name)| json!({"id": id, "name": name, "type": "Person", "disambiguation": ""}))
                .collect();
            (
                StatusCode::OK,
                json!({"count": found.len(), "offset": 0, "artists": found}),
            )
        }
        (_, Some(Reply::Discography(total, recordings))) => {
            let listed: Vec<Value> = recordings
                .iter()
                .enumerate()
                .map(|(index, (title, length))| {
                    json!({"id": format!("0f700000-0000-4000-8000-{:012x}", index + 1),
                        "title": title, "length": length, "video": false, "disambiguation": ""})
                })
                .collect();
            (
                StatusCode::OK,
                json!({"recording-count": total, "recording-offset": 0, "recordings": listed}),
            )
        }
        (_, Some(Reply::ReleaseGroups(groups))) => {
            let listed: Vec<Value> = groups
                .iter()
                .map(|(id, kind)| {
                    json!({"id": id, "title": "Owned fixture title", "primary-type": kind,
                        "secondary-types": [], "first-release-date": "2020"})
                })
                .collect();
            (
                StatusCode::OK,
                json!({"release-group-count": listed.len(), "release-group-offset": 0, "release-groups": listed}),
            )
        }
        (_, Some(Reply::Unavailable)) => {
            (StatusCode::SERVICE_UNAVAILABLE, json!({"error": "busy"}))
        }
        _ => (StatusCode::NOT_FOUND, json!({"error": "Not Found"})),
    };
    Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .body(Body::from(body.to_string()))
        .unwrap()
}
