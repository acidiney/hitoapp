use super::{MAX_REQUESTS, ObservedRequest, read_request};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{oneshot, watch},
    task::{JoinHandle, JoinSet},
};

struct Protocol {
    completed: Vec<u8>,
    expected_post: Vec<u8>,
    requests: Mutex<Vec<ObservedRequest>>,
    post_received: watch::Sender<bool>,
}

pub(in crate::tests) struct UncertainFixture {
    pub endpoint: reqwest::Url,
    protocol: Arc<Protocol>,
    post_received: watch::Receiver<bool>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl UncertainFixture {
    pub async fn start(completed: Vec<u8>, expected_post: Vec<u8>) -> Self {
        assert!(completed.len() <= 65_536);
        assert!(expected_post.len() <= 65_536);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the uncertain acknowledgement fixture must bind");
        let endpoint = format!(
            "http://{}/rest/1.0/",
            listener
                .local_addr()
                .expect("the uncertainty fixture must have an address")
        )
        .parse()
        .expect("the uncertainty fixture endpoint must be valid");
        let (received, post_received) = watch::channel(false);
        let protocol = Arc::new(Protocol {
            completed,
            expected_post,
            requests: Mutex::new(Vec::new()),
            post_received: received,
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
                                Duration::from_secs(2),
                                exchange(socket, index, serving),
                            ).await.expect("the owned uncertainty exchange must finish within its deadline");
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
                "an uncertainty fixture handler failed before joined shutdown"
            );
        });
        Self {
            endpoint,
            protocol,
            post_received,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub async fn wait_for_post_received(&mut self) -> Result<(), watch::error::RecvError> {
        loop {
            if *self.post_received.borrow_and_update() {
                return Ok(());
            }
            self.post_received.changed().await?;
        }
    }

    pub async fn close(mut self) -> Vec<ObservedRequest> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.expect("the owned uncertainty fixture must stop"),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    panic!("the owned uncertainty fixture did not join shutdown");
                }
            }
        }
        let mut requests = self
            .protocol
            .requests
            .lock()
            .expect("the uncertainty capture must lock");
        std::mem::take(&mut *requests)
    }
}

impl Drop for UncertainFixture {
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
            .expect("the uncertainty capture must lock");
        assert!(
            requests.len() < MAX_REQUESTS,
            "uncertainty request count exceeded its bound"
        );
        requests.push(request);
    }
    if index == 1 {
        socket
            .shutdown()
            .await
            .expect("the unacknowledged POST socket must close");
        drop(socket);
        if received_post {
            protocol.post_received.send_replace(true);
        }
        return;
    }
    let body = &protocol.completed;
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket
        .write_all(headers.as_bytes())
        .await
        .expect("the uncertainty fixture headers must be sent");
    socket
        .write_all(body)
        .await
        .expect("the uncertainty fixture body must be sent");
    socket
        .shutdown()
        .await
        .expect("the uncertainty fixture response socket must close");
}
