use super::fixtures::MEDIA_HOST;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::server::TlsStream;

pub struct MediaRequest {
    pub server_name: String,
    pub method: String,
    pub target: String,
    pub received_at: tokio::time::Instant,
    headers: Vec<(String, String)>,
}

impl MediaRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub struct MediaFixture {
    address: SocketAddr,
    ca_der: Vec<u8>,
    received: Arc<AtomicUsize>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<Vec<MediaRequest>>>,
}

impl MediaFixture {
    pub async fn start(wave: Vec<u8>) -> Self {
        assert!((44..=1_048_576).contains(&wave.len()));
        let (listener, tls, ca_der) = super::owned_tls::listener(MEDIA_HOST).await;
        let address = listener.local_addr().unwrap();
        let (stop, mut stopped) = oneshot::channel();
        let received = Arc::new(AtomicUsize::new(0));
        let counter = received.clone();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            loop {
                let socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                assert!(
                    requests.is_empty(),
                    "only one owned media request is authorized"
                );
                let exchange = async {
                    let mut socket = tls.accept(socket).await.unwrap();
                    let request = read_request(&mut socket).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nSet-Cookie: owned-private-media-cookie=private\r\nContent-Disposition: attachment; filename=\"owned-private-provider-name.wav\"\r\nConnection: close\r\n\r\n",
                        wave.len()
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(&wave).await.unwrap();
                    socket.shutdown().await.unwrap();
                    request
                };
                let request = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tokio::time::timeout(Duration::from_secs(5), exchange) => result.unwrap(),
                };
                requests.push(request);
            }
            requests
        });
        Self {
            address,
            ca_der,
            received,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn ca_der(&self) -> &[u8] {
        &self.ca_der
    }
    pub fn requests(&self) -> usize {
        self.received.load(Ordering::SeqCst)
    }

    pub async fn close(mut self) -> Vec<MediaRequest> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut server = self.server.take().unwrap();
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                server.abort();
                let _ = server.await;
                panic!("owned media did not stop");
            }
        }
    }
}

impl Drop for MediaFixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn read_request(socket: &mut TlsStream<TcpStream>) -> MediaRequest {
    let server_name = socket.get_ref().1.server_name().unwrap().to_owned();
    let mut frame = Vec::new();
    let end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        assert!(frame.len() < 8192);
        let mut byte = [0_u8; 1];
        socket.read_exact(&mut byte).await.unwrap();
        frame.push(byte[0]);
    };
    let mut lines = std::str::from_utf8(&frame[..end]).unwrap().split("\r\n");
    let line: Vec<_> = lines.next().unwrap().split(' ').collect();
    assert!(line.len() == 3 && line[0] == "GET" && line[2] == "HTTP/1.1");
    let headers: Vec<_> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    assert!(headers.len() <= 32 && !headers.iter().any(|(name, _)| name == "transfer-encoding"));
    let lengths: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name == "content-length")
        .collect();
    assert!(lengths.is_empty() || (lengths.len() == 1 && lengths[0].1 == "0"));
    MediaRequest {
        server_name,
        method: line[0].into(),
        target: line[1].into(),
        received_at: tokio::time::Instant::now(),
        headers,
    }
}
