use super::owned_tls;
use std::{net::SocketAddr, time::Duration};
use tokio::{io::AsyncWriteExt, sync::oneshot, task::JoinHandle};

pub struct ProviderFixture {
    address: SocketAddr,
    ca: Vec<u8>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<usize>>,
}
impl ProviderFixture {
    pub async fn start(replies: Vec<Option<Vec<u8>>>) -> Self {
        assert!(replies.len() <= 16 && replies.iter().flatten().all(|x| x.len() <= 65536));
        let (listener, tls, ca) = owned_tls::listener("api.torbox.app").await;
        let address = listener.local_addr().unwrap();
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut count = 0;
            let mut replies = replies.into_iter();
            loop {
                let socket =
                    tokio::select! {biased;_=&mut stopped=>break,r=listener.accept()=>r.unwrap().0};
                let reply = replies
                    .next()
                    .expect("no extra provider request is authorized");
                let exchange = async {
                    let mut socket = tls.accept(socket).await.unwrap();
                    let target = owned_tls::request(&mut socket, "api.torbox.app").await;
                    count += 1;
                    assert!(
                        target.starts_with("/v1/api/torrents/mylist?")
                            || target.starts_with("/v1/api/torrents/requestdl?")
                    );
                    if let Some(body) = reply {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        socket.write_all(head.as_bytes()).await.unwrap();
                        socket.write_all(&body).await.unwrap();
                    }
                    let _ = socket.shutdown().await;
                };
                tokio::select! {biased;_=&mut stopped=>break,r=tokio::time::timeout(Duration::from_secs(5),exchange)=>r.unwrap()};
            }
            count
        });
        Self {
            address,
            ca,
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
                panic!("provider fixture did not stop")
            }
        }
    }
}
impl Drop for ProviderFixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}
