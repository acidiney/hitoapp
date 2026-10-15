use std::{io, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub struct RawAudio {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}
impl RawAudio {
    pub async fn start(headers: &str, bytes: &[u8]) -> Self {
        let mut response = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\n{headers}Connection: close\r\n\r\n{:x}\r\n",bytes.len()).into_bytes();
        response.extend_from_slice(bytes);
        response.extend_from_slice(b"\r\n0\r\n\r\n");
        let response = Arc::new(response);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prefix/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    connection = listener.accept() => {
                        let (socket,_) = connection.unwrap();
                        let response = response.clone();
                        connections.spawn(async move {
                            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), exchange(socket,response)).await;
                        });
                    },
                    _ = connections.join_next(), if !connections.is_empty() => {},
                }
            }
        });
        Self { url, task }
    }
}
impl Drop for RawAudio {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn exchange(mut socket: TcpStream, audio: Arc<Vec<u8>>) -> io::Result<()> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") && header.len() < 16384 {
        header.push(socket.read_u8().await?);
    }
    let header = String::from_utf8(header).map_err(io::Error::other)?;
    let route = header.lines().next().unwrap_or_default();
    let length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if length > 16384 {
        return Err(io::Error::other("fixture request exceeds limit"));
    }
    socket.read_exact(&mut vec![0; length]).await?;
    if route.starts_with("GET /prefix/v1/audio?") {
        socket.write_all(&audio).await?;
        return Ok(());
    }
    let body = if route.contains("/v1/manifest ") {
        r#"{"id":"org.hito.raw-fixture","name":"Raw fixture","protocol_version":1,"capabilities":["search","sources"]}"#
    } else if route.contains("/v1/recordings/detail ") {
        r#"{"id":"recording","title":"Owned fixture","artist":"Fixture"}"#
    } else if route.contains("/v1/sources ") {
        r#"{"items":[{"id":"source","label":"Original","ready":true}]}"#
    } else {
        return Err(io::Error::other("unknown fixture route"));
    };
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
    Ok(())
}
