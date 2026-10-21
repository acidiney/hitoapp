use super::{entries, fixtures, owned_tls};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::server::TlsStream;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub requests: usize,
    pub mints: usize,
    fillers: usize,
}

#[derive(Default)]
pub struct Transcript {
    counts: Counts,
    ready_observations: Vec<tokio::time::Instant>,
}

pub struct Fixture {
    address: SocketAddr,
    ca: Vec<u8>,
    transcript: Arc<Mutex<Transcript>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Fixture {
    pub async fn start() -> Self {
        let (listener, tls, ca) = owned_tls::listener("api.torbox.app").await;
        let address = listener.local_addr().unwrap();
        let transcript = Arc::new(Mutex::new(Transcript::default()));
        let received = transcript.clone();
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
                    let target = request(&mut socket).await;
                    let body = {
                        let mut transcript = received.lock().unwrap();
                        assert!(
                            transcript.counts.requests < 1027,
                            "the owned capacity provider request bound was exceeded"
                        );
                        transcript.counts.requests += 1;
                        if target
                            == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false"
                        {
                            assert_eq!(
                                transcript.counts.mints, 0,
                                "retained audio must never remint"
                            );
                            assert_eq!(transcript.ready_observations.len(), 1);
                            transcript.counts.mints += 1;
                            fixtures::acknowledgment()
                        } else {
                            let torrent: u64 = target.strip_prefix("/v1/api/torrents/mylist?id=")
                                .and_then(|value| value.strip_suffix("&bypass_cache=true"))
                                .and_then(|value| value.parse().ok())
                                .expect("the owned capacity provider accepts only exact fresh observations");
                            if torrent == 7123 {
                                assert!(transcript.ready_observations.len() < 3);
                                transcript
                                    .ready_observations
                                    .push(tokio::time::Instant::now());
                                fixtures::observed(true, true, "album/owned.wav")
                            } else {
                                assert_eq!(transcript.counts.mints, 1);
                                let index = usize::try_from(
                                    torrent.checked_sub(entries::FIRST_FILLER_TORRENT).unwrap(),
                                )
                                .unwrap();
                                let binding = entries::binding(index);
                                assert_eq!(
                                    index, transcript.counts.fillers,
                                    "filler observations must match sequential public admissions"
                                );
                                transcript.counts.fillers += 1;
                                let mut body: serde_json::Value = serde_json::from_slice(
                                    &fixtures::observed(false, false, "album/owned.wav"),
                                )
                                .unwrap();
                                body["data"]["id"] = serde_json::json!(binding.torrent_id);
                                serde_json::to_vec(&body).unwrap()
                            }
                        }
                    };
                    assert!(body.len() < 8192);
                    let head = format!(
                        "HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                    let _ = socket.shutdown().await;
                };
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = tokio::time::timeout(Duration::from_secs(4), exchange) => result.expect("the owned capacity provider exchange exceeded its deadline"),
                }
            }
        });
        Self {
            address,
            ca,
            transcript,
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
    pub fn counts(&self) -> Counts {
        self.transcript.lock().unwrap().counts
    }

    pub async fn close(mut self) -> Transcript {
        drop(self.stop.take());
        let mut task = self.task.take().unwrap();
        tokio::time::timeout(Duration::from_secs(2), &mut task)
            .await
            .expect("the owned capacity provider must stop")
            .expect("the owned capacity provider must join");
        std::mem::take(&mut *self.transcript.lock().unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Transcript {
    pub fn assert_ready_retention(
        &self,
        accepted: usize,
        windows: &[(tokio::time::Instant, tokio::time::Instant)],
    ) {
        assert_eq!(
            self.counts,
            Counts {
                requests: accepted + 4,
                mints: 1,
                fillers: accepted
            }
        );
        assert_eq!(self.ready_observations.len(), 3);
        assert_eq!(windows.len(), 2);
        for (observed, (started, finished)) in self.ready_observations[1..].iter().zip(windows) {
            assert!(
                (started..=finished).contains(&observed),
                "each explicit retained GET must freshly observe the provider"
            );
        }
    }
}

async fn request(socket: &mut TlsStream<TcpStream>) -> String {
    assert!(socket.get_ref().1.server_name() == Some("api.torbox.app"));
    let mut frame = Vec::new();
    let end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        assert!(frame.len() < 8192);
        let mut byte = [0_u8; 1];
        socket.read_exact(&mut byte).await.unwrap();
        frame.push(byte[0]);
    };
    let mut lines = std::str::from_utf8(&frame[..end]).unwrap().split("\r\n");
    let line: Vec<_> = lines.next().unwrap().split(' ').collect();
    assert!(line.len() == 3 && line[0] == "GET" && line[2] == "HTTP/1.1");
    let headers: Vec<_> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim())
        })
        .collect();
    assert!(headers.len() <= 32);
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "host" && *value == "api.torbox.app")
    );
    for (name, value) in &headers {
        assert!(
            ![
                "cookie",
                "referer",
                "proxy-authorization",
                "x-hito-plugin-protocol",
                "transfer-encoding"
            ]
            .contains(&name.as_str())
        );
        assert!(name != "content-length" || *value == "0");
    }
    let authorization: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name == "authorization")
        .collect();
    if line[1].starts_with("/v1/api/torrents/mylist?") {
        assert!(
            authorization.len() == 1 && authorization[0].1 == format!("Bearer {}", fixtures::TOKEN)
        );
    } else {
        assert!(authorization.is_empty());
    }
    line[1].into()
}
