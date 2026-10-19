use crate::support::{Request, protocol::read_request};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

pub struct MediaFixture {
    address: SocketAddr,
    ca_der: Vec<u8>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<Vec<Request>>>,
}

impl MediaFixture {
    pub async fn start(wave: Vec<u8>) -> Self {
        assert!((44..=1_048_576).contains(&wave.len()));
        let _ = rustls::crypto::ring::default_provider().install_default();
        let key = KeyPair::generate().expect("the owned media CA key must generate");
        let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let root = ca.self_signed(&key).expect("the owned media CA must sign");
        let ca_der = root.der().to_vec();
        let issuer = Issuer::new(ca, key);
        let key = KeyPair::generate().unwrap();
        let certificate = CertificateParams::new(vec!["media.hito-fixture.invalid".into()])
            .unwrap()
            .signed_by(&key, &issuer)
            .expect("the owned media certificate must sign");
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
            )
            .expect("the owned media TLS endpoint must initialize");
        let tls = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            loop {
                let socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.expect("owned media must accept").0,
                };
                assert!(
                    requests.len() < 2,
                    "the owned media request bound was exceeded"
                );
                let request = tokio::time::timeout(Duration::from_secs(5), async {
                    let socket = tls
                        .accept(socket)
                        .await
                        .expect("owned media TLS must complete");
                    exchange(socket, &wave).await
                })
                .await
                .expect("the owned media exchange exceeded its deadline");
                requests.push(request);
            }
            requests
        });
        Self {
            address,
            ca_der,
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

    pub async fn close(mut self) -> Vec<Request> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut server = self.server.take().unwrap();
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(result) => result.expect("the owned media task must join"),
            Err(_) => {
                server.abort();
                let _ = server.await;
                panic!("the owned media task did not join within its deadline");
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

async fn exchange(mut socket: TlsStream<TcpStream>, wave: &[u8]) -> Request {
    let request = read_request(&mut socket, "GET").await;
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nSet-Cookie: owned-private-media-cookie=owned-private-media-cookie-value\r\nContent-Disposition: attachment; filename=\"owned-private-upstream-name.wav\"\r\nConnection: close\r\n\r\n",
        wave.len()
    );
    socket
        .write_all(head.as_bytes())
        .await
        .expect("owned WAV headers must send");
    socket
        .write_all(wave)
        .await
        .expect("the entire owned WAV body must send");
    socket
        .shutdown()
        .await
        .expect("the owned media TLS socket must close");
    request
}
