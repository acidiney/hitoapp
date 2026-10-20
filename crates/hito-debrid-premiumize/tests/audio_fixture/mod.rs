use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::TlsAcceptor;

pub mod logs;
mod protocol;
pub mod response;
pub use protocol::Request;
pub use response::{Reply, Response};

pub struct Fixture {
    address: SocketAddr,
    ca_der: Vec<u8>,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    pub async fn start(host: &'static str, replies: Vec<Reply>) -> Self {
        Self::responses(host, replies.into_iter().map(Response::from).collect()).await
    }

    pub async fn responses(host: &'static str, replies: Vec<Response>) -> Self {
        assert!(matches!(
            host,
            "www.premiumize.me" | "media.hito-fixture.invalid"
        ));
        assert!(!replies.is_empty() && replies.len() <= 8);
        assert!(replies.iter().all(|reply| reply.body.len() <= 400_000));
        let _ = rustls::crypto::ring::default_provider().install_default();
        let key = KeyPair::generate().expect("the owned TLS authority key must generate");
        let mut ca = CertificateParams::new(Vec::<String>::new())
            .expect("the owned TLS authority must configure");
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let root = ca
            .self_signed(&key)
            .expect("the owned TLS authority must sign");
        let ca_der = root.der().to_vec();
        let issuer = Issuer::new(ca, key);
        let key = KeyPair::generate().expect("the owned TLS endpoint key must generate");
        let certificate = CertificateParams::new(vec![host.into()])
            .expect("the owned TLS endpoint must configure")
            .signed_by(&key, &issuer)
            .expect("the owned TLS endpoint must sign");
        let tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
            )
            .expect("the owned TLS endpoint must initialize");
        let tls = TlsAcceptor::from(Arc::new(tls));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned TLS fixture must bind");
        let address = listener
            .local_addr()
            .expect("the owned TLS address must exist");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            let mut replies = replies.into_iter();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    completed = tasks.join_next(), if !tasks.is_empty() => {
                        completed.unwrap().expect("the owned TLS exchange must join");
                    },
                    accepted = listener.accept() => {
                        let socket = accepted.expect("the owned TLS fixture must accept").0;
                        let reply = replies.next().expect("the owned fixture received an extra connection");
                        let tls = tls.clone();
                        let recorded = recorded.clone();
                        assert!(tasks.len() < 8);
                        tasks.spawn(async move {
                            tokio::time::timeout(Duration::from_secs(12), async {
                                let mut socket = tls.accept(socket).await.expect("the owned TLS handshake must complete");
                                let request = protocol::read_request(&mut socket).await;
                                recorded.lock().unwrap().push(request);
                                reply.write(&mut socket).await;
                            }).await.expect("the owned TLS exchange exceeded its deadline");
                        });
                    }
                }
            }
            while let Some(completed) = tasks.join_next().await {
                completed.expect("the owned TLS exchange must join before fixture shutdown");
            }
        });
        Self {
            address,
            ca_der,
            requests,
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
    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub async fn close(mut self) -> Vec<Request> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut server = self.server.take().expect("the owned TLS task must exist");
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(result) => result.expect("the owned TLS task must join"),
            Err(_) => {
                server.abort();
                let _ = server.await;
                panic!("the owned TLS task did not join within its deadline");
            }
        }
        std::mem::take(&mut self.requests.lock().unwrap())
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
