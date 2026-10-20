use crate::support::{Request, protocol::read_request};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
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
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

pub struct HeldMedia {
    address: SocketAddr,
    ca_der: Vec<u8>,
    headers: [Arc<Notify>; 2],
    retired: [Arc<Notify>; 2],
    active: Arc<AtomicUsize>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<Vec<Request>>>,
}

impl HeldMedia {
    pub async fn start(wave: Vec<u8>, targets: [String; 3]) -> Self {
        assert!((44..=1_048_576).contains(&wave.len()));
        assert!(
            targets
                .iter()
                .all(|target| target.starts_with('/') && target.len() <= 2048)
        );
        assert!(targets[0] != targets[1] && targets[1] != targets[2] && targets[0] != targets[2]);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let key = KeyPair::generate().unwrap();
        let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let root = ca.self_signed(&key).unwrap();
        let ca_der = root.der().to_vec();
        let issuer = Issuer::new(ca, key);
        let key = KeyPair::generate().unwrap();
        let certificate = CertificateParams::new(vec!["media.hito-fixture.invalid".into()])
            .unwrap()
            .signed_by(&key, &issuer)
            .unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
            )
            .unwrap();
        let tls = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let headers = std::array::from_fn(|_| Arc::new(Notify::new()));
        let retired = std::array::from_fn(|_| Arc::new(Notify::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let (stop, mut stopped) = oneshot::channel();
        let wave = Arc::new(wave);
        let targets = Arc::new(targets);
        let task_headers = headers.clone();
        let task_retired = retired.clone();
        let task_active = active.clone();
        let server = tokio::spawn(async move {
            let mut exchanges = JoinSet::new();
            let mut accepted = 0;
            loop {
                let socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                assert!(
                    accepted < 3,
                    "only three owned media requests are authorized"
                );
                accepted += 1;
                let tls = tls.clone();
                let wave = wave.clone();
                let targets = targets.clone();
                let headers = task_headers.clone();
                let retired = task_retired.clone();
                let active = task_active.clone();
                exchanges.spawn(async move {
                    tokio::time::timeout(Duration::from_secs(20), async {
                        let socket = tls.accept(socket).await.unwrap();
                        exchange(socket, &wave, &targets, &headers, &retired, &active).await
                    })
                    .await
                    .expect("the owned held-media exchange exceeded its bound")
                });
            }
            let mut requests = Vec::new();
            while let Some(result) = exchanges.join_next().await {
                requests.push(result.expect("the owned held-media exchange must join"));
            }
            requests.sort_by_key(|request| request.received_at);
            requests
        });
        Self {
            address,
            ca_der,
            headers,
            retired,
            active,
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
    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
    pub async fn wait_for_headers(&self, index: usize) {
        tokio::time::timeout(Duration::from_secs(2), self.headers[index].notified())
            .await
            .expect("the actual owned WAV header must reach the peer");
    }
    pub async fn wait_for_eof(&self, index: usize) {
        tokio::time::timeout(Duration::from_secs(2), self.retired[index].notified())
            .await
            .expect("the cancelled body must retire its actual TLS peer");
    }
    pub async fn close(mut self) -> Vec<Request> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut server = self.server.take().unwrap();
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(result) => result.expect("the owned held-media server must join"),
            Err(_) => {
                server.abort();
                let _ = server.await;
                panic!("the owned held-media server failed to join");
            }
        }
    }
}

impl Drop for HeldMedia {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn exchange(
    mut socket: TlsStream<TcpStream>,
    wave: &[u8],
    targets: &[String; 3],
    headers: &[Arc<Notify>; 2],
    retired: &[Arc<Notify>; 2],
    active: &AtomicUsize,
) -> Request {
    let request = read_request(&mut socket, "GET").await;
    let index = targets
        .iter()
        .position(|target| *target == request.target)
        .expect("only the three exact owned media targets are authorized");
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nSet-Cookie: owned-private-media-cookie=owned-private-media-cookie-value\r\nContent-Disposition: attachment; filename=\"owned-private-upstream-name.wav\"\r\nConnection: close\r\n\r\n",
        wave.len()
    );
    socket.write_all(head.as_bytes()).await.unwrap();
    if index < 2 {
        socket.write_all(&wave[..44]).await.unwrap();
        socket.flush().await.unwrap();
        active.fetch_add(1, Ordering::SeqCst);
        headers[index].notify_one();
        let mut byte = [0_u8; 1];
        match socket.read(&mut byte).await {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
            _ => panic!("the held WAV must end through actual client EOF"),
        }
        active.fetch_sub(1, Ordering::SeqCst);
        retired[index].notify_one();
    } else {
        socket.write_all(wave).await.unwrap();
        socket.shutdown().await.unwrap();
    }
    request
}
