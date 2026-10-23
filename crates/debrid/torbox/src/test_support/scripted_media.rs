use super::{fixtures::MEDIA_HOST, owned_tls};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};

pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub declared: u64,
    pub body: Vec<u8>,
    pub closed: Option<oneshot::Sender<()>>,
    pub held_bytes: usize,
}
impl Reply {
    pub fn full(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: "audio/wav",
            declared: body.len() as u64,
            body,
            closed: None,
            held_bytes: 44,
        }
    }
    pub fn held(body: Vec<u8>) -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        let mut reply = Self::full(body);
        reply.closed = Some(tx);
        (reply, rx)
    }
}
pub struct ScriptedMedia {
    address: SocketAddr,
    ca: Vec<u8>,
    received: Arc<AtomicUsize>,
    notify: Arc<tokio::sync::Notify>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<usize>>,
}
impl ScriptedMedia {
    pub async fn start(replies: Vec<Reply>) -> Self {
        assert!(
            !replies.is_empty()
                && replies.len() <= 4
                && replies.iter().all(|r| r.body.len() <= 1_048_576)
        );
        let (listener, tls, ca) = owned_tls::listener(MEDIA_HOST).await;
        let address = listener.local_addr().unwrap();
        let (stop, mut stopped) = oneshot::channel();
        let received = Arc::new(AtomicUsize::new(0));
        let counter = received.clone();
        let notify = Arc::new(tokio::sync::Notify::new());
        let witness = notify.clone();
        let server = tokio::spawn(async move {
            let mut replies = replies.into_iter();
            let mut tasks = JoinSet::new();
            let mut accepted = 0;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tasks.join_next(), if !tasks.is_empty() => {
                        result.unwrap().unwrap();
                    },
                    socket = listener.accept() => {
                        let socket = socket.unwrap().0;
                        let reply = replies.next().expect("no extra media request is authorized");
                        accepted += 1;
                        let tls = tls.clone();
                        let count = counter.clone();
                        let observed = witness.clone();
                        tasks.spawn(async move {
                            tokio::time::timeout(Duration::from_secs(8), async {
                                let mut socket = tls.accept(socket).await.unwrap();
                                let target = owned_tls::request(&mut socket, MEDIA_HOST).await;
                                assert!(target == super::fixtures::MEDIA_TARGET);
                                let head = format!(
                                    "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\nContent-Length: {}\r\nLocation: https://unapproved.invalid/never-follow\r\nConnection: close\r\n\r\n",
                                    reply.status, reply.content_type, reply.declared
                                );
                                if socket.write_all(head.as_bytes()).await.is_err() {
                                    return;
                                }
                                count.fetch_add(1, Ordering::SeqCst);
                                observed.notify_waiters();
                                if let Some(closed) = reply.closed {
                                    socket.write_all(&reply.body[..reply.held_bytes]).await.unwrap();
                                    owned_tls::peer_closed(&mut socket).await;
                                    let _ = closed.send(());
                                } else {
                                    let _ = socket.write_all(&reply.body).await;
                                    let _ = socket.shutdown().await;
                                }
                            }).await.expect("owned media handler exceeded deadline");
                        });
                    }
                }
            }
            tasks.abort_all();
            while let Some(result) = tasks.join_next().await {
                assert!(result.is_ok() || result.unwrap_err().is_cancelled());
            }
            accepted
        });
        Self {
            address,
            ca,
            received,
            notify,
            stop: Some(stop),
            server: Some(server),
        }
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn ca_der(&self) -> &[u8] {
        &self.ca
    }
    pub fn requests(&self) -> usize {
        self.received.load(Ordering::SeqCst)
    }
    pub async fn wait_for_requests(&self, count: usize) {
        loop {
            let changed = self.notify.notified();
            if self.requests() >= count {
                return;
            }
            changed.await;
        }
    }

    pub async fn close(mut self) -> usize {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut server = self.server.take().unwrap();
        match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
            Ok(r) => r.unwrap(),
            Err(_) => {
                server.abort();
                let _ = server.await;
                panic!("media fixture did not stop")
            }
        }
    }
}
impl Drop for ScriptedMedia {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}
