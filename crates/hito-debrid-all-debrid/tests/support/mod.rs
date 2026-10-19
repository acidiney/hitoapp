use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::{collections::VecDeque, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

pub mod logs;

pub struct Request {
    pub server_name: String,
    pub method: String,
    pub target: String,
    pub body: Vec<u8>,
    headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub struct Fixture {
    address: SocketAddr,
    ca_der: Vec<u8>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<Vec<Request>>>,
}

pub enum Reply {
    Json(Vec<u8>),
    DisconnectAfterRequest,
}

impl From<Vec<u8>> for Reply {
    fn from(body: Vec<u8>) -> Self {
        Self::Json(body)
    }
}

impl Fixture {
    pub async fn start(replies: Vec<impl Into<Reply>>) -> Self {
        let replies: Vec<Reply> = replies.into_iter().map(Into::into).collect();
        assert!(
            replies.len() <= 8
                && replies.iter().all(|reply| match reply {
                    Reply::Json(body) => body.len() <= 65_536,
                    Reply::DisconnectAfterRequest => true,
                })
        );
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
        let certificate = CertificateParams::new(vec!["api.alldebrid.com".into()])
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
            .expect("the owned TLS fixture must have an address");
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut replies = VecDeque::from(replies);
            let mut requests = Vec::new();
            loop {
                let socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.expect("the owned TLS fixture must accept").0,
                };
                assert!(
                    requests.len() < 8,
                    "the owned TLS request bound was exceeded"
                );
                let reply = replies.pop_front().unwrap_or(Reply::DisconnectAfterRequest);
                let exchange = async {
                    let socket = tls
                        .accept(socket)
                        .await
                        .expect("the owned TLS handshake must complete");
                    exchange(socket, reply).await
                };
                let request = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tokio::time::timeout(Duration::from_secs(5), exchange) => result.expect("the owned TLS exchange exceeded its deadline"),
                };
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
        let mut server = self.server.take().expect("the owned TLS task must exist");
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(result) => result.expect("the owned TLS task must join"),
            Err(_) => {
                server.abort();
                let _ = server.await;
                panic!("the owned TLS task did not join within its deadline");
            }
        }
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

async fn exchange(mut socket: TlsStream<TcpStream>, reply: Reply) -> Request {
    let server_name = socket
        .get_ref()
        .1
        .server_name()
        .expect("the owned TLS request must have SNI")
        .to_owned();
    let mut frame = Vec::new();
    let end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        assert!(
            frame.len() < 8192,
            "the owned request headers exceeded their bound"
        );
        let mut byte = [0_u8; 1];
        socket
            .read_exact(&mut byte)
            .await
            .expect("the owned request headers must complete");
        frame.push(byte[0]);
    };
    let head = std::str::from_utf8(&frame[..end]).expect("the owned request headers must be UTF-8");
    let mut lines = head.split("\r\n");
    let line: Vec<_> = lines
        .next()
        .expect("the owned request line must exist")
        .split(' ')
        .collect();
    assert!(
        line.len() == 3 && line[2] == "HTTP/1.1",
        "the owned request framing changed"
    );
    let method = line[0].to_owned();
    let target = line[1].to_owned();
    let headers: Vec<_> = lines
        .map(|line| {
            let (key, value) = line
                .split_once(':')
                .expect("the owned request header must have a value");
            (key.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    assert!(headers.len() <= 32 && !headers.iter().any(|(name, _)| name == "transfer-encoding"));
    let lengths: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name == "content-length")
        .collect();
    assert!(
        lengths.len() == 1,
        "the owned request body length must be unambiguous"
    );
    let length = lengths[0]
        .1
        .parse::<usize>()
        .expect("the owned request body length must be numeric");
    assert!(length <= 4096, "the owned request body exceeded its bound");
    let mut body = vec![0; length];
    socket
        .read_exact(&mut body)
        .await
        .expect("the owned request body must complete");
    if let Reply::Json(body) = reply {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket
            .write_all(head.as_bytes())
            .await
            .expect("the owned response headers must be sent");
        socket
            .write_all(&body)
            .await
            .expect("the owned response body must be sent");
    }
    socket
        .shutdown()
        .await
        .expect("the owned TLS socket must close");
    Request {
        server_name,
        method,
        target,
        body,
        headers,
    }
}
