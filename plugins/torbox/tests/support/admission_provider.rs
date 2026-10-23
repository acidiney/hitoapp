use super::{admission_workflow::selection, fixtures, owned_tls};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, sync::oneshot, task::JoinHandle};

pub enum Step {
    Observe(usize),
    Mint(usize),
}

pub struct Provider {
    address: SocketAddr,
    ca_der: Vec<u8>,
    requests: Arc<AtomicUsize>,
    mints: Arc<AtomicUsize>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Provider {
    pub async fn start(steps: Vec<Step>) -> Self {
        assert!(steps.len() <= 16);
        let (listener, tls, ca_der) = owned_tls::listener("api.torbox.app").await;
        let address = listener.local_addr().unwrap();
        let (stop, mut stopped) = oneshot::channel();
        let requests = Arc::new(AtomicUsize::new(0));
        let mints = Arc::new(AtomicUsize::new(0));
        let received = requests.clone();
        let minted = mints.clone();
        let task = tokio::spawn(async move {
            let mut steps = steps.into_iter();
            loop {
                let socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                let step = steps
                    .next()
                    .expect("no extra provider request is authorized");
                let exchange = async {
                    let mut socket = tls.accept(socket).await.unwrap();
                    let target = owned_tls::request(&mut socket, "api.torbox.app").await;
                    let body = match step {
                        Step::Observe(index) => {
                            let binding = selection(index).binding;
                            assert!(
                                target
                                    == format!(
                                        "/v1/api/torrents/mylist?id={}&bypass_cache=true",
                                        binding.torrent_id
                                    )
                            );
                            let mut body: serde_json::Value = serde_json::from_slice(
                                &fixtures::observed(true, true, "album/owned.wav"),
                            )
                            .unwrap();
                            body["data"]["id"] = serde_json::json!(binding.torrent_id);
                            body["data"]["hash"] = serde_json::json!(binding.info_hash);
                            serde_json::to_vec(&body).unwrap()
                        }
                        Step::Mint(index) => {
                            let binding = selection(index).binding;
                            assert!(
                                target
                                    == format!(
                                        "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id={}&file_id={}&zip_link=false&redirect=false&append_name=false",
                                        binding.torrent_id, binding.file_id
                                    )
                            );
                            minted.fetch_add(1, Ordering::SeqCst);
                            fixtures::acknowledgment()
                        }
                    };
                    received.fetch_add(1, Ordering::SeqCst);
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                    let _ = socket.shutdown().await;
                };
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tokio::time::timeout(Duration::from_secs(4), exchange) => result.unwrap(),
                }
            }
        });
        Self {
            address,
            ca_der,
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
        &self.ca_der
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
                panic!("the owned provider did not stop");
            }
        }
        self.requests()
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
