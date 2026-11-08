#[allow(dead_code)]
#[path = "source_resolution.rs"]
mod source;

pub use source::{CHOSEN, TARGET};

use serde_json::{Value, json};
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, oneshot},
    task::{JoinHandle, JoinSet},
};

pub const WAITING: &str = "owned-waiting-recording";
pub const WAITING_CANDIDATE: &str = "owned-waiting-candidate";
const WAITING_TITLE: &str = "Owned waiting recording";

struct State {
    searches: AtomicUsize,
    candidate_reads: AtomicUsize,
    entered: Semaphore,
    aborted: Semaphore,
    release: Semaphore,
    released: Semaphore,
}

impl Default for State {
    fn default() -> Self {
        Self {
            searches: AtomicUsize::new(0),
            candidate_reads: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            aborted: Semaphore::new(0),
            release: Semaphore::new(0),
            released: Semaphore::new(0),
        }
    }
}

pub struct Provider {
    pub url: String,
    source: source::Provider,
    state: Arc<State>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl Provider {
    pub async fn start() -> Self {
        let source = source::Provider::start().await;
        let upstream = source
            .url
            .trim_start_matches("http://")
            .trim_end_matches('/');
        let upstream: SocketAddr = upstream.parse().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let state = Arc::new(State::default());
        let exchanges = state.clone();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            let mut healthy = true;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    joined = connections.join_next(), if !connections.is_empty() => {
                        healthy &= matches!(joined, Some(Ok(Ok(()))));
                    }
                    accepted = listener.accept() => {
                        let Ok((socket, _)) = accepted else { return false; };
                        connections.spawn(exchange(socket, upstream, exchanges.clone()));
                    }
                }
            }
            connections.abort_all();
            while let Some(joined) = connections.join_next().await {
                healthy &= match joined {
                    Ok(Ok(())) => true,
                    Err(error) => error.is_cancelled(),
                    _ => false,
                };
            }
            healthy
        });
        Self {
            url,
            source,
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn counts(&self) -> ((usize, usize, usize), usize, usize) {
        (
            self.source.counts(),
            self.state.searches.load(Ordering::SeqCst),
            self.state.candidate_reads.load(Ordering::SeqCst),
        )
    }

    pub async fn search_entered(&self) {
        self.state.entered.acquire().await.unwrap().forget();
    }

    pub async fn search_aborted(&self) {
        self.state.aborted.acquire().await.unwrap().forget();
    }

    pub fn release_search(&self) {
        self.state.release.close();
    }

    pub async fn search_released(&self) {
        self.state.released.acquire().await.unwrap().forget();
    }

    pub async fn finish(&mut self) -> bool {
        self.release_search();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let joined = match self.task.take() {
            Some(mut task) => match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
                Ok(result) => matches!(result, Ok(true)),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    false
                }
            },
            None => true,
        };
        self.source.finish().await && joined
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.release_search();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn exchange(
    mut socket: TcpStream,
    upstream: SocketAddr,
    state: Arc<State>,
) -> io::Result<()> {
    let (method, path, body) = request(&mut socket).await?;
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if method == "POST"
        && path == "/v1/search"
        && value
            == json!({"query": format!("{} {WAITING_TITLE}", source::ARTIST), "cursor": null, "limit": 20})
    {
        state.searches.fetch_add(1, Ordering::SeqCst);
        state.entered.add_permits(1);
        let mut extra = [0];
        let aborted = tokio::select! {
            _ = state.release.acquire() => false,
            read = socket.read(&mut extra) => match read {
                Ok(0) | Err(_) => true,
                Ok(_) => return Err(io::Error::other("unexpected bytes after provider request")),
            },
        };
        if aborted {
            state.aborted.add_permits(1);
            let _ = state.release.acquire().await;
        }
        let result = reply(
            &mut socket,
            json!({"items": [recording(WAITING_CANDIDATE)], "next_cursor": null}),
        )
        .await;
        state.released.add_permits(1);
        return if aborted { Ok(()) } else { result };
    }
    if method == "POST"
        && [WAITING, WAITING_CANDIDATE].contains(&value["recording_id"].as_str().unwrap_or(""))
    {
        let id = value["recording_id"].as_str().unwrap();
        if id == WAITING_CANDIDATE {
            state.candidate_reads.fetch_add(1, Ordering::SeqCst);
        }
        match path.as_str() {
            "/v1/recordings/detail" => return reply(&mut socket, recording(id)).await,
            "/v1/sources" => {
                let sources = if id == WAITING {
                    json!({"items": []})
                } else {
                    json!({"items": [{"id": "owned-waiting-audio", "label": "Owned waiting audio", "ready": true}]})
                };
                return reply(&mut socket, sources).await;
            }
            _ => {}
        }
    }
    let mut upstream_socket = TcpStream::connect(upstream).await?;
    upstream_socket.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {upstream}\r\nConnection: close\r\nX-Hito-Plugin-Protocol: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await?;
    upstream_socket.write_all(&body).await?;
    let mut response = Vec::new();
    upstream_socket.read_to_end(&mut response).await?;
    socket.write_all(&response).await
}

fn recording(id: &str) -> Value {
    json!({"id": id, "title": WAITING_TITLE, "artist": source::ARTIST, "duration_ms": 12000})
}

async fn reply(socket: &mut TcpStream, body: Value) -> io::Result<()> {
    let body = body.to_string();
    socket.write_all(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await
}

async fn request(socket: &mut TcpStream) -> io::Result<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let split = loop {
        let mut chunk = [0; 1024];
        let read = socket.read(&mut chunk).await?;
        if read == 0 || bytes.len() + read > 16_384 {
            return Err(io::Error::other("provider request framing is invalid"));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break split;
        }
    };
    let head = std::str::from_utf8(&bytes[..split])
        .map_err(|_| io::Error::other("provider request headers are invalid"))?;
    let mut lines = head.split("\r\n");
    let mut route = lines.next().unwrap_or_default().split_whitespace();
    let method = route.next().unwrap_or_default().to_owned();
    let path = route.next().unwrap_or_default().to_owned();
    let mut length = 0;
    let mut protocol = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = value
                .trim()
                .parse()
                .map_err(|_| io::Error::other("provider body length is invalid"))?;
        }
        if name.eq_ignore_ascii_case("x-hito-plugin-protocol") {
            protocol = value.trim() == "1";
        }
    }
    if !protocol || length > 8192 {
        return Err(io::Error::other("provider request protocol is invalid"));
    }
    let mut body = bytes[split + 4..].to_vec();
    if body.len() > length {
        return Err(io::Error::other("provider request contains extra bytes"));
    }
    let received = body.len();
    body.resize(length, 0);
    socket.read_exact(&mut body[received..]).await?;
    Ok((method, path, body))
}
