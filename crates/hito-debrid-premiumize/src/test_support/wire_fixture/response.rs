use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
};
use tokio_rustls::server::TlsStream;

pub struct Reply {
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    gate: Option<Gate>,
    prefix: Option<usize>,
}

pub struct Witness {
    pub reached: oneshot::Receiver<()>,
    pub peer_closed: oneshot::Receiver<()>,
    pub release: oneshot::Sender<()>,
}

struct Gate {
    reached: oneshot::Sender<()>,
    peer_closed: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

impl From<Reply> for Response {
    fn from(reply: Reply) -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type".into(), reply.content_type.into()),
                ("Content-Length".into(), reply.body.len().to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
                (
                    "Set-Cookie".into(),
                    "owned-private-media-cookie=owned-private-cookie-value".into(),
                ),
                (
                    "Content-Disposition".into(),
                    "attachment; filename=\"owned-private-upstream-name.wav\"".into(),
                ),
            ],
            body: reply.body,
            gate: None,
            prefix: None,
        }
    }
}

impl Response {
    pub fn json(body: Vec<u8>) -> Self {
        Reply {
            content_type: "application/json",
            body,
        }
        .into()
    }
    pub fn wave(body: Vec<u8>) -> Self {
        Reply {
            content_type: "audio/wav",
            body,
        }
        .into()
    }

    pub fn held(mut self, prefix: Option<usize>) -> (Self, Witness) {
        if let Some(prefix) = prefix {
            assert!(prefix <= self.body.len());
        }
        let (reached, observed) = oneshot::channel();
        let (peer_closed, closed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        self.gate = Some(Gate {
            reached,
            peer_closed,
            release: released,
        });
        self.prefix = prefix;
        (
            self,
            Witness {
                reached: observed,
                peer_closed: closed,
                release,
            },
        )
    }

    pub(super) async fn write(mut self, socket: &mut TlsStream<TcpStream>) {
        if self.prefix.is_none()
            && let Some(gate) = self.gate.take()
            && !gate.wait(socket).await
        {
            return;
        }
        assert!(self.headers.len() <= 40);
        let mut head = format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\n", self.status);
        for (name, value) in &self.headers {
            assert!(!name.contains(['\r', '\n']) && !value.contains(['\r', '\n']));
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        assert!(head.len() <= 16_384);
        if socket.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        let prefix = self.prefix.unwrap_or(0);
        if socket.write_all(&self.body[..prefix]).await.is_err() {
            return;
        }
        if let Some(gate) = self.gate.take()
            && !gate.wait(socket).await
        {
            return;
        }
        let _ = socket.write_all(&self.body[prefix..]).await;
        let _ = socket.shutdown().await;
    }
}

impl Gate {
    async fn wait(self, socket: &mut TlsStream<TcpStream>) -> bool {
        self.reached
            .send(())
            .expect("the owned gate observer must remain live");
        let mut byte = [0_u8; 1];
        tokio::select! {
            result = self.release => {
                result.expect("the owned response gate must release explicitly");
                true
            },
            result = socket.read(&mut byte) => {
                match result {
                    Ok(0) => {},
                    Err(error) if matches!(error.kind(),
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::NotConnected) => {},
                    _ => panic!("the owned peer did not close its TLS transport"),
                }
                self.peer_closed.send(()).expect("the peer-close observer must remain live");
                false
            }
        }
    }
}
