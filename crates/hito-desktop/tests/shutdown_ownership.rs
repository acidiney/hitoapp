use hito_desktop::{Error, LocalHost, Paths};
use reqwest::Client;
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpSocket,
};

fn paths(directory: &Path) -> Paths {
    Paths {
        data: directory.join("data"),
        music: directory.join("music"),
        web: directory.join("web"),
    }
}

async fn exercise(cancel_waiter: bool) {
    let directory = tempfile::tempdir().unwrap();
    let assets = directory.path().join("web/assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::File::create(assets.join("app.js"))
        .unwrap()
        .set_len(8 * 1024 * 1024)
        .unwrap();
    let host = LocalHost::start(paths(directory.path())).await.unwrap();
    let entry = host.entry_url();
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client.get(entry.clone()).send().await.unwrap();
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let address: SocketAddr = format!("127.0.0.1:{}", entry.port().unwrap())
        .parse()
        .unwrap();
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(1024).unwrap();
    let mut socket = socket.connect(address).await.unwrap();
    socket
        .write_all(
            format!("GET /assets/app.js HTTP/1.1\r\nHost: {address}\r\nCookie: {cookie}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut header = [0; 128];
    socket.read_exact(&mut header).await.unwrap();
    assert!(header.starts_with(b"HTTP/1.1 200"));
    if cancel_waiter {
        let waiting = tokio::spawn(host.shutdown());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !waiting.is_finished(),
            "The response must still be draining"
        );
        waiting.abort();
        assert!(waiting.await.unwrap_err().is_cancelled());
    } else {
        drop(host);
    }
    assert!(
        matches!(
            LocalHost::start(paths(directory.path())).await,
            Err(Error::AlreadyRunning)
        ),
        "A draining host must retain its library claim"
    );
    let replacement = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match LocalHost::start(paths(directory.path())).await {
                Ok(host) => break host,
                Err(Error::AlreadyRunning) => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(error) => panic!("Replacement failed: {error}"),
            }
        }
    })
    .await
    .expect("The old lifecycle must finish without its observer");
    drop(socket);
    replacement.shutdown().await.unwrap();
}

#[tokio::test]
async fn dropping_the_host_retains_its_claim_until_a_slow_response_is_reclaimed() {
    exercise(false).await;
}

#[tokio::test]
async fn cancelling_a_shutdown_waiter_cannot_release_the_library_claim_early() {
    exercise(true).await;
}
