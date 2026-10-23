use axum::{
    Router,
    body::{Body, Bytes},
    routing::get,
};
use std::{
    convert::Infallible,
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

struct Released(Option<oneshot::Sender<()>>);
impl Drop for Released {
    fn drop(&mut self) {
        let _ = self.0.take().unwrap().send(());
    }
}

async fn exercise(body: bool, abort: bool) {
    let directory = tempfile::tempdir().unwrap();
    let core = hito_core::open(hito_core::CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        hito_core::HostDeviceId::new("runtime-test").unwrap(),
    ))
    .await
    .unwrap();
    let (started, starting) = oneshot::channel();
    let (released, release) = oneshot::channel();
    let signals = Arc::new(Mutex::new(Some((started, released))));
    let app = Router::new().route(
        "/held",
        get(move || {
            let signals = signals.clone();
            async move {
                let (started, released) = signals.lock().unwrap().take().unwrap();
                let held = Released(Some(released));
                started.send(()).unwrap();
                if !body {
                    pending::<()>().await;
                }
                Body::from_stream(futures_util::stream::unfold(held, |held| async move {
                    pending::<()>().await;
                    Some((Ok::<_, Infallible>(Bytes::from_static(b"audio")), held))
                }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(hito_server::serve(listener, app, core, async {
        let _ = stopped.await;
    }));
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket
        .write_all(format!("GET /held HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), starting)
        .await
        .unwrap()
        .unwrap();
    if abort {
        serving.abort();
        assert!(serving.await.unwrap_err().is_cancelled());
    } else {
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(7), serving)
            .await
            .expect("Shutdown must bound an open connection")
            .unwrap()
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(7), release)
        .await
        .expect("Shutdown must release the in-flight handler or body")
        .unwrap();
    let mut remainder = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), socket.read_to_end(&mut remainder))
        .await
        .unwrap()
        .unwrap();
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn shutdown_reclaims_pending_handlers_and_audio_bodies() {
    exercise(false, false).await;
    exercise(true, false).await;
}

#[tokio::test]
async fn cancelling_the_host_reclaims_its_open_connections() {
    exercise(false, true).await;
    exercise(true, true).await;
}
