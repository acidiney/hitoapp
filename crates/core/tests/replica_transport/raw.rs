use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    task::JoinHandle,
};

pub struct RawServer {
    pub endpoint: String,
    received: Arc<Notify>,
    pub closed: Arc<AtomicBool>,
    owner: Option<JoinHandle<()>>,
    held: bool,
}

impl RawServer {
    pub async fn start(response: Vec<u8>, hold: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://localhost:{}", listener.local_addr().unwrap().port());
        let received = Arc::new(Notify::new());
        let closed = Arc::new(AtomicBool::new(false));
        let observed = received.clone();
        let closed_observed = closed.clone();
        let owner = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let byte = socket.read_u8().await.unwrap();
                bytes.push(byte);
                assert!(bytes.len() <= 16_384);
                if bytes.ends_with(b"\r\n\r\n") {
                    break bytes.len();
                }
            };
            let headers = std::str::from_utf8(&bytes).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            assert!(length <= 1_048_576);
            bytes.resize(header_end + length, 0);
            socket.read_exact(&mut bytes[header_end..]).await.unwrap();
            observed.notify_one();
            socket.write_all(&response).await.unwrap();
            socket.flush().await.unwrap();
            if hold {
                let mut byte = [0];
                let count = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut byte))
                    .await
                    .expect("cancelled or bounded request retained its connection")
                    .unwrap();
                assert_eq!(count, 0, "client unexpectedly reused a retired connection");
                closed_observed.store(true, Ordering::SeqCst);
            }
        });
        Self {
            endpoint,
            received,
            closed,
            owner: Some(owner),
            held: hold,
        }
    }

    pub async fn received(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.received.notified())
            .await
            .unwrap();
    }
    pub async fn close(mut self) {
        let owner = self.owner.take().unwrap();
        tokio::time::timeout(Duration::from_secs(4), owner)
            .await
            .unwrap()
            .unwrap();
        if self.held {
            assert!(self.closed.load(Ordering::SeqCst));
        }
    }
}

impl Drop for RawServer {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            owner.abort();
        }
    }
}
