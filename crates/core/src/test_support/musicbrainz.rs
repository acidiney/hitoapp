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
    /// A search page with explicit relevance scores and a stated total.
    Scored(Vec<(String, String, u32)>, u32),
    /// The first page of an artist's recordings: the total MusicBrainz states, then each title
    /// with its length in milliseconds.
    Discography(u32, Vec<(String, Option<u64>)>),
    /// One browse page of release groups, each with its identifier and primary type.
    ReleaseGroups(Vec<(String, Option<String>)>),
    /// The official genres of one entity, each with its name and vote count.
    Genres(Vec<(String, u32)>),
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

    /// Answers an artist search with explicit relevance scores and a total beyond the page.
    pub fn search_scored(&self, name: &str, found: Vec<(String, String, u32)>, total: u32) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("search/{name}"), Reply::Scored(found, total));
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

    /// Answers the genre lookup of one artist, release group or recording.
    pub fn genres_of(&self, entity: &str, id: &str, reply: Reply) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("genres/{entity}/{id}"), reply);
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

fn parameter(uri: &Uri, name: &str) -> Option<String> {
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
}

fn window(uri: &Uri) -> (usize, usize) {
    let number = |name: &str| parameter(uri, name).and_then(|value| value.parse().ok());
    (number("offset").unwrap_or(0), number("limit").unwrap_or(25))
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn search_page(uri: &Uri, artists: &[(String, String, u32)], total: u32) -> Value {
    let (offset, limit) = window(uri);
    let found: Vec<Value> = artists
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(id, name, score)| json!({"id": id, "name": name, "type": "Person", "disambiguation": "", "score": score}))
        .collect();
    json!({"count": total, "offset": offset, "artists": found})
}

fn key(uri: &Uri) -> Option<String> {
    let path = uri.path().strip_prefix("/ws/2/")?;
    if let Some(isrc) = path.strip_prefix("isrc/") {
        return Some(format!("isrc/{isrc}"));
    }
    if let Some((entity, id)) = path.split_once('/')
        && matches!(entity, "artist" | "release-group" | "recording")
        && uri
            .query()
            .is_some_and(|query| query.contains("inc=genres"))
    {
        return Some(format!("genres/{entity}/{id}"));
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

/// Exact-name artists score 100 and sort first, like MusicBrainz phrase searches.
fn scored(key: &str, artists: &[(String, String)]) -> Vec<(String, String, u32)> {
    let wanted = normalized(key.trim_start_matches("search/"));
    let mut found: Vec<(String, String, u32)> = artists
        .iter()
        .map(|(id, name)| {
            let score = if normalized(name) == wanted { 100 } else { 70 };
            (id.clone(), name.clone(), score)
        })
        .collect();
    found.sort_by(|a, b| b.2.cmp(&a.2));
    found
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
        (Some(key), Some(Reply::CandidatesOf(artists, total))) => {
            let scored = scored(&key, &artists);
            (StatusCode::OK, search_page(request.uri(), &scored, total))
        }
        (Some(key), Some(Reply::Candidates(artists))) => {
            let scored = scored(&key, &artists);
            let total = scored.len() as u32;
            (StatusCode::OK, search_page(request.uri(), &scored, total))
        }
        (_, Some(Reply::Scored(artists, total))) => {
            (StatusCode::OK, search_page(request.uri(), &artists, total))
        }
        (_, Some(Reply::Discography(total, recordings))) => {
            let (offset, limit) = window(request.uri());
            let listed: Vec<Value> = recordings
                .iter()
                .enumerate()
                .skip(offset)
                .take(limit)
                .map(|(index, (title, length))| {
                    json!({"id": format!("0f700000-0000-4000-8000-{:012x}", index + 1),
                        "title": title, "length": length, "video": false, "disambiguation": ""})
                })
                .collect();
            (
                StatusCode::OK,
                json!({"recording-count": total, "recording-offset": offset, "recordings": listed}),
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
        (Some(key), Some(Reply::Genres(genres))) => {
            let listed: Vec<Value> = genres
                .iter()
                .map(|(name, count)| json!({"id": "0f700000-0000-4000-8000-0000000000d1", "name": name, "count": count, "disambiguation": ""}))
                .collect();
            let id = key.rsplit('/').next().unwrap_or_default();
            (
                StatusCode::OK,
                json!({"id": id, "genres": listed, "tags": [{"name": "not a genre", "count": 3}]}),
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
