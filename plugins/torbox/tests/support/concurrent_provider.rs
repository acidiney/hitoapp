use super::{fixtures, owned_tls, provider::reply::Reply};
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

pub enum Exchange {
    Observe(Reply),
    Mint(Reply),
}

pub struct ConcurrentProvider {
    address: SocketAddr,
    ca: Vec<u8>,
    requests: Arc<AtomicUsize>,
    mints: Arc<AtomicUsize>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl ConcurrentProvider {
    pub async fn start(exchanges: Vec<Exchange>) -> Self {
        assert!(exchanges.len() <= 8);
        let (listener, tls, ca) = owned_tls::listener("api.torbox.app").await;
        let address = listener.local_addr().unwrap();
        let (stop, mut stopped) = oneshot::channel();
        let requests = Arc::new(AtomicUsize::new(0));
        let mints = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        let mint_count = mints.clone();
        let task = tokio::spawn(async move {
            let mut exchanges = exchanges.into_iter();
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    completed = tasks.join_next(), if !tasks.is_empty() => { completed.unwrap().unwrap(); },
                    accepted = listener.accept() => {
                        assert!(tasks.len() < 4, "owned provider concurrency exceeded its bound");
                        let socket = accepted.unwrap().0;
                        let exchange = exchanges.next().expect("no extra provider request is authorized");
                        let tls = tls.clone();
                        let reads = request_count.clone();
                        let mints = mint_count.clone();
                        tasks.spawn(async move {
                            tokio::time::timeout(Duration::from_secs(8), async {
                                let mut socket = tls.accept(socket).await.unwrap();
                                let target = owned_tls::request(&mut socket, "api.torbox.app").await;
                                let mut reply = match exchange {
                                    Exchange::Observe(reply) => {
                                        assert!(target == "/v1/api/torrents/mylist?id=7123&bypass_cache=true");
                                        reply
                                    }
                                    Exchange::Mint(reply) => {
                                        assert!(target == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false");
                                        mints.fetch_add(1, Ordering::SeqCst);
                                        reply
                                    }
                                };
                                reads.fetch_add(1, Ordering::SeqCst);
                                if let Some((received, release)) = reply.gate.take() {
                                    received.send(()).unwrap();
                                    assert!(reply.peer_closed.is_none());
                                    release.await.unwrap();
                                }
                                let mut head = format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\n", reply.status);
                                assert!(reply.headers.len() <= 32 && reply.body.len() <= 65_536);
                                for (name, value) in reply.headers {
                                    assert!(!name.contains(['\r', '\n']) && !value.contains(['\r', '\n']));
                                    head.push_str(&format!("{name}: {value}\r\n"));
                                }
                                head.push_str("\r\n");
                                if socket.write_all(head.as_bytes()).await.is_ok() {
                                    let _ = socket.write_all(&reply.body).await;
                                    let _ = socket.shutdown().await;
                                }
                            }).await.expect("owned concurrent provider exchange exceeded its deadline");
                        });
                    }
                }
            }
            tasks.abort_all();
            while let Some(result) = tasks.join_next().await {
                assert!(result.is_ok() || result.unwrap_err().is_cancelled());
            }
        });
        Self {
            address,
            ca,
            requests,
            mints,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn ca_der(&self) -> &[u8] {
        &self.ca
    }
    pub fn requests(&self) -> (usize, usize) {
        (
            self.requests.load(Ordering::SeqCst),
            self.mints.load(Ordering::SeqCst),
        )
    }

    pub async fn close(mut self) -> (usize, usize) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let mut task = self.task.take().unwrap();
        match tokio::time::timeout(Duration::from_secs(1), &mut task).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                task.abort();
                let _ = task.await;
                panic!("owned concurrent provider did not stop");
            }
        }
        self.requests()
    }
}

impl Drop for ConcurrentProvider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub fn completed() -> Exchange {
    Exchange::Observe(Reply::json(fixtures::observed(
        true,
        true,
        "album/owned.wav",
    )))
}
