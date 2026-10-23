use super::{MAX_REQUESTS, ObservedRequest, read_request};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
    task::{JoinHandle, JoinSet},
};

pub(in crate::tests) struct ConcurrentFixture {
    pub endpoint: reqwest::Url,
    requests: Arc<Mutex<Vec<ObservedRequest>>>,
    started: watch::Receiver<usize>,
    disconnected: watch::Receiver<usize>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl ConcurrentFixture {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the concurrent owned provider fixture must bind");
        let endpoint = format!(
            "http://{}/rest/1.0/",
            listener
                .local_addr()
                .expect("the concurrent fixture must have an address")
        )
        .parse()
        .expect("the concurrent fixture endpoint must be valid");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let (started, starts) = watch::channel(0_usize);
        let (disconnected, disconnects) = watch::channel(0_usize);
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            let mut accepted_count = 0;
            let mut failed = false;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        if !matches!(result, Some(Ok(()))) {
                            failed = true;
                            break;
                        }
                    },
                    accepted = listener.accept(), if accepted_count < MAX_REQUESTS => {
                        let Ok((socket, _)) = accepted else {
                            failed = true;
                            break;
                        };
                        accepted_count += 1;
                        let observed = observed.clone();
                        let started = started.clone();
                        let disconnected = disconnected.clone();
                        handlers.spawn(async move {
                            tokio::time::timeout(
                                Duration::from_secs(2),
                                hold_response(socket, observed, started, disconnected),
                            ).await.expect("the concurrent owned exchange must finish within its deadline");
                        });
                    },
                }
            }
            handlers.abort_all();
            while let Some(result) = handlers.join_next().await {
                if result.is_err_and(|error| !error.is_cancelled()) {
                    failed = true;
                }
            }
            assert!(
                !failed,
                "a concurrent fixture handler failed before joined shutdown"
            );
        });
        Self {
            endpoint,
            requests,
            started: starts,
            disconnected: disconnects,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub async fn wait_for_starts(&mut self, count: usize) -> Result<(), watch::error::RecvError> {
        wait_for_count(&mut self.started, count).await
    }

    pub async fn wait_for_disconnects(
        &mut self,
        count: usize,
    ) -> Result<(), watch::error::RecvError> {
        wait_for_count(&mut self.disconnected, count).await
    }

    pub async fn close(mut self) -> Vec<ObservedRequest> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.expect("the concurrent owned fixture must stop"),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    panic!("the concurrent owned fixture did not join shutdown");
                }
            }
        }
        let mut requests = self
            .requests
            .lock()
            .expect("the concurrent capture must lock");
        std::mem::take(&mut *requests)
    }
}

impl Drop for ConcurrentFixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn hold_response(
    mut socket: TcpStream,
    observed: Arc<Mutex<Vec<ObservedRequest>>>,
    started: watch::Sender<usize>,
    disconnected: watch::Sender<usize>,
) {
    let request = read_request(&mut socket).await;
    {
        let mut requests = observed.lock().expect("the concurrent capture must lock");
        assert!(
            requests.len() < MAX_REQUESTS,
            "concurrent request count exceeded its bound"
        );
        requests.push(request);
    }
    socket.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n"
    ).await.expect("the concurrent fixture headers must be sent");
    started.send_modify(|count| *count += 1);
    let mut byte = [0_u8; 1];
    let received = socket
        .read(&mut byte)
        .await
        .expect("the concurrent fixture must observe the client connection");
    assert_eq!(
        received, 0,
        "the client must close the concurrent held response"
    );
    disconnected.send_modify(|count| *count += 1);
    socket
        .shutdown()
        .await
        .expect("the concurrent fixture socket must close");
}

async fn wait_for_count(
    receiver: &mut watch::Receiver<usize>,
    count: usize,
) -> Result<(), watch::error::RecvError> {
    loop {
        let observed = *receiver.borrow_and_update();
        if observed >= count {
            return Ok(());
        }
        receiver.changed().await?;
    }
}
