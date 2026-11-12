use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
    task::JoinHandle,
};

pub struct ProtocolUpstream {
    pub url: String,
    pub started: Option<oneshot::Receiver<()>>,
    task: Option<JoinHandle<()>>,
}

impl ProtocolUpstream {
    pub async fn start(reply: Vec<u8>, hold: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (started_tx, started) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 8192);
                connection.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let _ = connection.write_all(&reply).await;
            let _ = started_tx.send(());
            if hold {
                match connection.read(&mut byte).await {
                    Ok(0) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                    other => panic!("owned upstream response did not retire: {other:?}"),
                }
            }
            let _ = connection.shutdown().await;
        });
        Self {
            url,
            started: Some(started),
            task: Some(task),
        }
    }

    pub async fn close(mut self) {
        tokio::time::timeout(Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}

impl Drop for ProtocolUpstream {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
