use super::{fixtures, owned_tls};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, sync::oneshot, task::JoinHandle};

pub struct CapabilityProvider {
    address: SocketAddr,
    ca: Vec<u8>,
    counts: Arc<Mutex<(usize, usize)>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl CapabilityProvider {
    pub async fn start(observation_delay: Duration) -> Self {
        assert!(observation_delay <= Duration::from_secs(1));
        let (listener, tls, ca) = owned_tls::listener("api.torbox.app").await;
        let address = listener.local_addr().unwrap();
        let counts = Arc::new(Mutex::new((0, 0)));
        let recorded = counts.clone();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                let socket = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                let exchange = async {
                    let mut socket = tls.accept(socket).await.unwrap();
                    let target = owned_tls::request(&mut socket, "api.torbox.app").await;
                    let (mint, body) = if target
                        == "/v1/api/torrents/mylist?id=7123&bypass_cache=true"
                    {
                        (false, fixtures::observed(true, true, "album/owned.wav"))
                    } else {
                        assert!(
                            target
                                == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false",
                            "the owned provider received an unauthorized request"
                        );
                        (true, fixtures::acknowledgment())
                    };
                    {
                        let mut counts = recorded.lock().unwrap();
                        assert!(
                            counts.0 < 8,
                            "the owned provider request bound was exceeded"
                        );
                        counts.0 += 1;
                        counts.1 += usize::from(mint);
                    }
                    if !mint {
                        tokio::time::sleep(observation_delay).await;
                    }
                    let head = format!(
                        "HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    if socket.write_all(head.as_bytes()).await.is_ok() {
                        let _ = socket.write_all(&body).await;
                        let _ = socket.shutdown().await;
                    }
                };
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tokio::time::timeout(Duration::from_secs(2), exchange) => {
                        result.expect("the owned provider exchange exceeded its deadline");
                    }
                }
            }
        });
        Self {
            address,
            ca,
            counts,
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

    pub fn counts(&self) -> (usize, usize) {
        *self.counts.lock().unwrap()
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
                panic!("the owned provider did not stop within its deadline");
            }
        }
        self.counts()
    }
}

impl Drop for CapabilityProvider {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
