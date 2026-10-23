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

struct Protocol {
    completed: Vec<u8>,
    expected_post: Vec<u8>,
    get_delay: Duration,
    requests: Mutex<Vec<ObservedRequest>>,
    post_ready: watch::Sender<bool>,
    post_eof: watch::Sender<bool>,
}

pub(in crate::tests) struct DeadlineFixture {
    pub endpoint: reqwest::Url,
    protocol: Arc<Protocol>,
    post_ready: watch::Receiver<bool>,
    post_eof: watch::Receiver<bool>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl DeadlineFixture {
    pub async fn start(completed: Vec<u8>, expected_post: Vec<u8>, get_delay: Duration) -> Self {
        assert!(completed.len() <= 65_536 && expected_post.len() <= 65_536);
        assert!(get_delay <= Duration::from_secs(7));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned unrestriction deadline fixture must bind");
        let endpoint = format!(
            "http://{}/rest/1.0/",
            listener
                .local_addr()
                .expect("the deadline fixture must have an address")
        )
        .parse()
        .expect("the deadline fixture endpoint must be valid");
        let (ready, post_ready) = watch::channel(false);
        let (eof, post_eof) = watch::channel(false);
        let protocol = Arc::new(Protocol {
            completed,
            expected_post,
            get_delay,
            requests: Mutex::new(Vec::new()),
            post_ready: ready,
            post_eof: eof,
        });
        let serving = protocol.clone();
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
                        let index = accepted_count;
                        accepted_count += 1;
                        let serving = serving.clone();
                        handlers.spawn(async move {
                            tokio::time::timeout(
                                Duration::from_secs(14),
                                exchange(socket, index, serving),
                            ).await.expect("the owned deadline exchange must finish within its bound");
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
                "a deadline fixture handler failed before joined shutdown"
            );
        });
        Self {
            endpoint,
            protocol,
            post_ready,
            post_eof,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub async fn wait_for_post_ready(&mut self) -> Result<(), watch::error::RecvError> {
        wait_for_true(&mut self.post_ready).await
    }

    pub async fn wait_for_post_eof(&mut self) -> Result<(), watch::error::RecvError> {
        wait_for_true(&mut self.post_eof).await
    }

    pub async fn close(mut self) -> Vec<ObservedRequest> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.expect("the owned deadline fixture must stop"),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    panic!("the owned deadline fixture did not join shutdown");
                }
            }
        }
        let mut requests = self
            .protocol
            .requests
            .lock()
            .expect("the deadline capture must lock");
        std::mem::take(&mut *requests)
    }
}

impl Drop for DeadlineFixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

async fn exchange(mut socket: TcpStream, index: usize, protocol: Arc<Protocol>) {
    let request = read_request(&mut socket).await;
    let received_post = request.method == "POST"
        && request.target == "/rest/1.0/unrestrict/link"
        && request.body == protocol.expected_post;
    {
        let mut requests = protocol
            .requests
            .lock()
            .expect("the deadline capture must lock");
        assert!(
            requests.len() < MAX_REQUESTS,
            "deadline request count exceeded its bound"
        );
        requests.push(request);
    }
    if index == 1 {
        socket.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n"
        ).await.expect("the held POST response headers must be sent");
        if received_post {
            protocol.post_ready.send_replace(true);
        }
        let mut byte = [0_u8; 1];
        let received = socket
            .read(&mut byte)
            .await
            .expect("the fixture must observe the held POST peer");
        assert_eq!(received, 0, "the held POST connection must reach peer EOF");
        socket
            .shutdown()
            .await
            .expect("the held POST response socket must close");
        drop(socket);
        protocol.post_eof.send_replace(true);
        return;
    }
    if index == 0 && !protocol.get_delay.is_zero() {
        tokio::time::sleep(protocol.get_delay).await;
    }
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        protocol.completed.len()
    ).into_bytes();
    response.extend_from_slice(&protocol.completed);
    socket
        .write_all(&response)
        .await
        .expect("the owned completed torrent response must be sent");
    socket
        .shutdown()
        .await
        .expect("the owned metadata response socket must close");
}

async fn wait_for_true(
    receiver: &mut watch::Receiver<bool>,
) -> Result<(), watch::error::RecvError> {
    loop {
        if *receiver.borrow_and_update() {
            return Ok(());
        }
        receiver.changed().await?;
    }
}
