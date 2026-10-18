use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
    task::JoinHandle,
};

const MAX_REQUEST_BYTES: usize = 8192;
const MAX_REQUESTS: usize = 8;

mod concurrent;
pub(super) use concurrent::ConcurrentFixture;

enum Reply {
    Json(Vec<u8>),
    HeldBody(u64),
    HeldChunkedBody(Vec<u8>),
    ChunkedBody(Vec<u8>),
}

pub(super) struct ObservedRequest {
    pub method: String,
    pub target: String,
    headers: Vec<(String, String)>,
    pub buffered_body_bytes: usize,
    pub received_at: Instant,
}

impl ObservedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub(super) struct Fixture {
    pub endpoint: reqwest::Url,
    requests: Arc<Mutex<Vec<ObservedRequest>>>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
    held_body_started: watch::Receiver<bool>,
    held_body_disconnected: watch::Receiver<bool>,
}

impl Fixture {
    pub async fn start(replies: Vec<Vec<u8>>) -> Self {
        Self::start_replies(replies.into_iter().map(Reply::Json).collect()).await
    }

    pub async fn with_held_body(declared_length: u64) -> Self {
        Self::start_replies(vec![Reply::HeldBody(declared_length)]).await
    }

    pub async fn with_held_body_then_json(declared_length: u64, body: Vec<u8>) -> Self {
        Self::start_replies(vec![Reply::HeldBody(declared_length), Reply::Json(body)]).await
    }

    pub async fn with_held_chunked_body(body: Vec<u8>) -> Self {
        Self::start_replies(vec![Reply::HeldChunkedBody(body)]).await
    }

    pub async fn with_chunked_body(body: Vec<u8>) -> Self {
        Self::start_replies(vec![Reply::ChunkedBody(body)]).await
    }

    async fn start_replies(replies: Vec<Reply>) -> Self {
        assert!(replies.len() <= MAX_REQUESTS);
        assert!(replies.iter().all(|reply| match reply {
            Reply::Json(body) => body.len() <= 65_536,
            Reply::HeldBody(length) => *length > 0,
            Reply::HeldChunkedBody(body) => body.len() == 65_537,
            Reply::ChunkedBody(body) => !body.is_empty() && body.len() <= 65_536,
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned provider fixture must bind");
        let endpoint = format!(
            "http://{}/rest/1.0/",
            listener
                .local_addr()
                .expect("the fixture must have an address")
        )
        .parse()
        .expect("the owned provider endpoint must be valid");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let (stop, mut stopped) = oneshot::channel();
        let (held_started, held_body_started) = watch::channel(false);
        let (held_disconnected, held_body_disconnected) = watch::channel(false);
        let server = tokio::spawn(async move {
            let mut replies = VecDeque::from(replies);
            loop {
                let mut socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.expect("the fixture must accept").0,
                };
                let exchange = async {
                    let request = read_request(&mut socket).await;
                    {
                        let mut requests = observed.lock().expect("the request capture must lock");
                        assert!(
                            requests.len() < MAX_REQUESTS,
                            "provider request count exceeded its bound"
                        );
                        requests.push(request);
                    }
                    let (status, reply) = match replies.pop_front() {
                        Some(reply) => ("200 OK", reply),
                        None => ("500 Internal Server Error", Reply::Json(b"{}".to_vec())),
                    };
                    let framing = match &reply {
                        Reply::Json(body) => format!("Content-Length: {}", body.len()),
                        Reply::HeldBody(length) => format!("Content-Length: {length}"),
                        Reply::HeldChunkedBody(_) | Reply::ChunkedBody(_) => {
                            "Transfer-Encoding: chunked".into()
                        }
                    };
                    let headers = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{framing}\r\nConnection: close\r\n\r\n"
                    );
                    socket
                        .write_all(headers.as_bytes())
                        .await
                        .expect("the fixture headers must be sent");
                    if matches!(&reply, Reply::HeldBody(_) | Reply::HeldChunkedBody(_)) {
                        held_started.send_replace(true);
                    }
                    match reply {
                        Reply::Json(body) => socket
                            .write_all(&body)
                            .await
                            .expect("the fixture body must be sent"),
                        Reply::HeldBody(_) => {
                            wait_for_disconnect(&mut socket, &held_disconnected).await;
                        }
                        Reply::HeldChunkedBody(body) => {
                            write_chunks(&mut socket, &body).await;
                            wait_for_disconnect(&mut socket, &held_disconnected).await;
                        }
                        Reply::ChunkedBody(body) => {
                            write_chunks(&mut socket, &body).await;
                            socket
                                .write_all(b"0\r\n\r\n")
                                .await
                                .expect("the fixture chunk terminator must be sent");
                        }
                    }
                    socket
                        .shutdown()
                        .await
                        .expect("the fixture socket must close");
                };
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tokio::time::timeout(Duration::from_secs(2), exchange) => {
                        result.expect("the owned provider exchange must finish within its deadline");
                    },
                }
            }
        });
        Self {
            endpoint,
            requests,
            stop: Some(stop),
            server: Some(server),
            held_body_started,
            held_body_disconnected,
        }
    }

    pub fn request_count(&self) -> usize {
        self.requests
            .lock()
            .expect("the request capture must lock")
            .len()
    }

    pub async fn wait_for_held_body_disconnect(&mut self) -> Result<(), watch::error::RecvError> {
        loop {
            let disconnected = *self.held_body_disconnected.borrow_and_update();
            if disconnected {
                return Ok(());
            }
            self.held_body_disconnected.changed().await?;
        }
    }

    pub async fn wait_for_held_body_start(&mut self) -> Result<(), watch::error::RecvError> {
        loop {
            let started = *self.held_body_started.borrow_and_update();
            if started {
                return Ok(());
            }
            self.held_body_started.changed().await?;
        }
    }

    pub async fn close(mut self) -> Vec<ObservedRequest> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.expect("the owned provider fixture must stop"),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    panic!("the owned provider fixture did not join shutdown");
                }
            }
        }
        let mut requests = self.requests.lock().expect("the request capture must lock");
        std::mem::take(&mut *requests)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn write_chunks(socket: &mut TcpStream, body: &[u8]) {
    for chunk in body.chunks(65_536) {
        let length = chunk.len();
        let chunk_header = format!("{length:x}\r\n");
        let mut frame = Vec::with_capacity(chunk_header.len() + length + 2);
        frame.extend_from_slice(chunk_header.as_bytes());
        frame.extend_from_slice(chunk);
        frame.extend_from_slice(b"\r\n");
        socket
            .write_all(&frame)
            .await
            .expect("the fixture chunk frame must be sent");
    }
}

async fn wait_for_disconnect(socket: &mut TcpStream, disconnected: &watch::Sender<bool>) {
    let mut byte = [0_u8; 1];
    let received = socket
        .read(&mut byte)
        .await
        .expect("the fixture must observe the client connection");
    assert_eq!(received, 0, "the client must close the held response");
    disconnected.send_replace(true);
}

async fn read_request(socket: &mut TcpStream) -> ObservedRequest {
    let mut frame = Vec::new();
    let header_end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        let mut chunk = [0_u8; 1024];
        let length = socket
            .read(&mut chunk)
            .await
            .expect("the fixture request must be readable");
        assert!(length > 0, "the fixture request ended before its headers");
        assert!(
            length <= MAX_REQUEST_BYTES.saturating_sub(frame.len()),
            "provider request frame exceeded its limit"
        );
        frame.extend_from_slice(&chunk[..length]);
    };
    let headers =
        std::str::from_utf8(&frame[..header_end]).expect("the owned request headers must be UTF-8");
    let received_at = Instant::now();
    let mut lines = headers.split("\r\n");
    let mut request_line = lines
        .next()
        .expect("the request line must exist")
        .split(' ');
    let method = request_line
        .next()
        .expect("the method must exist")
        .to_owned();
    let target = request_line
        .next()
        .expect("the target must exist")
        .to_owned();
    assert_eq!(request_line.next(), Some("HTTP/1.1"));
    assert!(request_line.next().is_none());
    let headers = lines
        .map(|line| {
            let (name, value) = line
                .split_once(':')
                .expect("the request header must have a value");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    ObservedRequest {
        method,
        target,
        headers,
        buffered_body_bytes: frame.len() - header_end - 4,
        received_at,
    }
}
