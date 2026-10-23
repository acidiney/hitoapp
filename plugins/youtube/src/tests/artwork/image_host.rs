use hito_testkit::tls::OwnedAuthority;
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

pub(super) const HOST: &str = "i.ytimg.com";

#[derive(Clone)]
pub(super) struct Received {
    pub method: String,
    pub target: String,
    pub headers: BTreeMap<String, String>,
}

pub(super) struct Reply {
    status: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    pub fn jpeg(body: &[u8]) -> Self {
        Self::raw("200 OK", &[("Content-Type", "image/jpeg")], body)
    }

    pub fn raw(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Self {
        Self {
            status: status.into(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).into(), (*value).into()))
                .collect(),
            body: body.to_vec(),
        }
    }
}

pub(super) struct Host {
    received: Arc<Mutex<Vec<Received>>>,
    client: crate::media::Client,
    server: JoinHandle<()>,
}

impl Host {
    pub async fn start(reply: Reply) -> Self {
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
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = received.clone();
        let reply = Arc::new(reply);
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                let log = log.clone();
                let reply = reply.clone();
                tokio::spawn(async move {
                    let Ok(Ok(mut stream)) =
                        tokio::time::timeout(Duration::from_secs(2), acceptor.accept(socket)).await
                    else {
                        return;
                    };
                    let Ok(request) = read_request(&mut stream).await else {
                        return;
                    };
                    log.lock().unwrap().push(request);
                    let mut head = format!("HTTP/1.1 {}\r\n", reply.status);
                    for (name, value) in &reply.headers {
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                    head.push_str(&format!(
                        "Content-Length: {}\r\nConnection: close\r\n\r\n",
                        reply.body.len()
                    ));
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(&reply.body).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            received,
            client: crate::media::Client::for_fixture(tls, HOST, address),
            server,
        }
    }

    pub fn client(&self) -> crate::media::Client {
        self.client.clone()
    }

    pub fn requests(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn read_request(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> io::Result<Received> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await?;
        if count == 0 || bytes.len() + count > 8192 {
            return Err(io::Error::other("Invalid owned image request"));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| io::Error::other("Invalid headers"))?;
    let mut lines = text.split("\r\n");
    let mut start = lines.next().unwrap().split_whitespace();
    let method = start.next().unwrap().to_owned();
    let target = start.next().unwrap().to_owned();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    Ok(Received {
        method,
        target,
        headers,
    })
}
