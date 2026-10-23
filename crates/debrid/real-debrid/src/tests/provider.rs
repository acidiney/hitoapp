use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
    task::JoinHandle,
};

const MAX_REQUESTS: usize = 8;

mod concurrent;
mod deadline;
mod request;
mod uncertainty;
pub(super) use concurrent::ConcurrentFixture;
pub(super) use deadline::DeadlineFixture;
pub(super) use request::ObservedRequest;
use request::read_request;
pub(super) use uncertainty::UncertainFixture;

enum Reply {
    Json(Vec<u8>),
    StatusJson(u16, Vec<u8>),
    HeldBody(u64),
    HeldChunkedBody(Vec<u8>),
    ChunkedBody(Vec<u8>),
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

    pub async fn with_acknowledgement(torrent: Vec<u8>, status: u16, body: Vec<u8>) -> Self {
        Self::start_replies(vec![Reply::Json(torrent), Reply::StatusJson(status, body)]).await
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
            Reply::StatusJson(status, body) => {
                matches!(status, 200 | 401 | 403 | 500) && body.len() <= 65_536
            }
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
                        Some(reply) => {
                            let status = match &reply {
                                Reply::StatusJson(401, _) => "401 Unauthorized",
                                Reply::StatusJson(403, _) => "403 Forbidden",
                                Reply::StatusJson(500, _) => "500 Internal Server Error",
                                _ => "200 OK",
                            };
                            (status, reply)
                        }
                        None => ("500 Internal Server Error", Reply::Json(b"{}".to_vec())),
                    };
                    let framing = match &reply {
                        Reply::Json(body) | Reply::StatusJson(_, body) => {
                            format!("Content-Length: {}", body.len())
                        }
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
                        Reply::Json(body) | Reply::StatusJson(_, body) => socket
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
