use hito_testkit::tls::OwnedAuthority;
use std::{
    collections::BTreeMap,
    io,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::server::TlsStream;
use tokio_util::sync::CancellationToken;

pub(super) const HOST: &str = "rr1---sn-hito.googlevideo.com";

#[derive(Clone)]
pub(super) struct Received {
    pub method: String,
    pub target: String,
    pub headers: BTreeMap<String, String>,
}

struct State {
    audio: Vec<u8>,
    received: Mutex<Vec<Received>>,
    held: AtomicUsize,
    close_timeout: AtomicBool,
    closed: Semaphore,
    written: Semaphore,
    stopped: CancellationToken,
}

pub(super) struct Fixture {
    state: Arc<State>,
    client: crate::media::Client,
    server: JoinHandle<()>,
    address: SocketAddr,
}

impl Fixture {
    pub async fn start(audio: Vec<u8>) -> Self {
        let authority = OwnedAuthority::new();
        let acceptor = authority.acceptor(HOST);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(
                authority.root_der.clone(),
            ))
            .unwrap();
        let client_tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let client = crate::media::Client::for_fixture(client_tls, HOST, address);
        let state = Arc::new(State {
            audio,
            received: Mutex::new(Vec::new()),
            held: AtomicUsize::new(0),
            close_timeout: AtomicBool::new(true),
            closed: Semaphore::new(0),
            written: Semaphore::new(0),
            stopped: CancellationToken::new(),
        });
        let owned = state.clone();
        let server = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = owned.stopped.cancelled() => break,
                    accepted = listener.accept() => {
                        assert!(connections.len() < 32, "Owned CDN exceeded its connection bound");
                        let (socket, _) = accepted.unwrap();
                        let acceptor = acceptor.clone();
                        let state = owned.clone();
                        connections.spawn(async move {
                            let stream = tokio::time::timeout(
                                Duration::from_secs(2), acceptor.accept(socket),
                            ).await.unwrap().unwrap();
                            serve(stream, state).await
                        });
                    }
                    result = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap().unwrap().unwrap();
                    }
                }
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap().unwrap();
            }
        });
        Self {
            state,
            client,
            server,
            address,
        }
    }

    pub fn client(&self) -> crate::media::Client {
        self.client.clone()
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn url(&self) -> String {
        let expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 600;
        format!(
            "https://{HOST}/videoplayback?expire={expires}&lmt=1730000000000000&sig=private-signed-url-sentinel"
        )
    }

    pub fn hold(&self, count: usize) {
        assert_eq!(self.state.held.swap(count, Ordering::SeqCst), 0);
    }

    pub fn hold_until_closed(&self, count: usize) {
        self.state.close_timeout.store(false, Ordering::SeqCst);
        self.hold(count);
    }

    pub async fn closed(&self, count: u32) {
        tokio::time::timeout(
            Duration::from_secs(2),
            self.state.closed.acquire_many(count),
        )
        .await
        .expect("Dropping a downstream body did not close the upstream connection")
        .unwrap()
        .forget();
    }

    pub fn received(&self) -> Vec<Received> {
        self.state.received.lock().unwrap().clone()
    }

    pub async fn written(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.state.written.acquire())
            .await
            .expect("The owned CDN did not finish writing its representation")
            .unwrap()
            .forget();
    }

    pub async fn shutdown(&mut self) {
        self.state.stopped.cancel();
        tokio::time::timeout(Duration::from_secs(3), &mut self.server)
            .await
            .unwrap()
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.state.stopped.cancel();
        self.server.abort();
    }
}

async fn serve(mut stream: TlsStream<tokio::net::TcpStream>, state: Arc<State>) -> io::Result<()> {
    let request = tokio::time::timeout(Duration::from_secs(2), read_request(&mut stream))
        .await
        .map_err(|_| io::Error::other("Owned CDN request timed out"))??;
    {
        let mut received = state.received.lock().unwrap();
        assert!(received.len() < 32, "Owned CDN exceeded its request bound");
        received.push(request.clone());
    }
    assert!(request.target.starts_with("/videoplayback?"));
    let total = state.audio.len();
    let range = request.headers.get("range");
    let selected = range.map(|value| {
        let (first, last) = value
            .strip_prefix("bytes=")
            .unwrap()
            .split_once('-')
            .unwrap();
        let first: usize = first.parse().unwrap();
        let last = if last.is_empty() {
            total - 1
        } else {
            last.parse().unwrap()
        };
        (first, last)
    });
    if selected.is_some_and(|(first, last)| first >= total || last < first) {
        stream.write_all(format!(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ).as_bytes()).await?;
        let _ = stream.shutdown().await;
        return Ok(());
    }
    let (first, last) = selected.unwrap_or((0, total - 1));
    let status = if selected.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let mut headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: audio/mp4\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"owned-aac-v1\"\r\nConnection: close\r\n",
        last - first + 1,
    );
    if selected.is_some() {
        headers.push_str(&format!("Content-Range: bytes {first}-{last}/{total}\r\n"));
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).await?;
    if request.method == "HEAD" {
        let _ = stream.shutdown().await;
        return Ok(());
    }
    assert_eq!(request.method, "GET");
    let hold = selected.is_none() && {
        let mut remaining = state.held.load(Ordering::SeqCst);
        loop {
            let Some(next) = remaining.checked_sub(1) else {
                break false;
            };
            match state.held.compare_exchange_weak(
                remaining,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break true,
                Err(observed) => remaining = observed,
            }
        }
    };
    if hold {
        stream.write_all(&state.audio[..64]).await?;
        stream.flush().await?;
        let mut byte = [0];
        let closed = async {
            if state.close_timeout.load(Ordering::SeqCst) {
                matches!(
                    tokio::time::timeout(Duration::from_secs(5), stream.read(&mut byte)).await,
                    Ok(Ok(0)) | Ok(Err(_))
                )
            } else {
                matches!(stream.read(&mut byte).await, Ok(0) | Err(_))
            }
        };
        let disconnected = tokio::select! {
            _ = state.stopped.cancelled() => false,
            result = closed => result,
        };
        assert!(
            disconnected,
            "Owned upstream remained open after downstream cancellation"
        );
        state.closed.add_permits(1);
        return Ok(());
    }
    stream.write_all(&state.audio[first..=last]).await?;
    stream.flush().await?;
    state.written.add_permits(1);
    let _ = stream.shutdown().await;
    Ok(())
}

async fn read_request(stream: &mut TlsStream<tokio::net::TcpStream>) -> io::Result<Received> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await?;
        if count == 0 || bytes.len() + count > 8192 {
            return Err(io::Error::other("Invalid owned CDN request"));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| io::Error::other("Invalid owned headers"))?;
    let mut lines = text.split("\r\n");
    let mut start = lines.next().unwrap().split_whitespace();
    let method = start.next().unwrap().to_owned();
    let target = start.next().unwrap().to_owned();
    assert_eq!(start.next(), Some("HTTP/1.1"));
    let mut headers = BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        assert!(headers.len() < 32);
        let (name, value) = line.split_once(':').unwrap();
        assert!(
            headers
                .insert(name.to_ascii_lowercase(), value.trim().to_owned())
                .is_none()
        );
    }
    Ok(Received {
        method,
        target,
        headers,
    })
}
