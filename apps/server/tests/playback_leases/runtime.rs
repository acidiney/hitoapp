use super::fixture::Case;
use axum::http::StatusCode;
use hito_core::PlaybackLease;
use http_body_util::BodyExt;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

#[tokio::test]
async fn tcp_shutdown_retires_leases_and_unpolled_bodies_before_owned_storage_cleanup() {
    let f = Case::with_audio(|path| {
        let mut bytes = hito_core::test_support::support::write_wav(path, 73);
        bytes.resize(32 * 1024 * 1024 + 44, 0);
        let data_length = (bytes.len() - 44) as u32;
        bytes[4..8].copy_from_slice(&(data_length + 36).to_le_bytes());
        bytes[40..44].copy_from_slice(&data_length.to_le_bytes());
        std::fs::write(path, &bytes).unwrap();
        bytes
    })
    .await;
    let response = f.begin().await;
    assert_eq!(response.status(), StatusCode::OK);
    let lease: PlaybackLease =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let path = format!("/api/v1/playback/leases/{}/audio", lease.id);
    let retained = f.request("GET", &path, &[], None).await;
    assert_eq!(retained.status(), StatusCode::OK);
    let mut retained_body = retained.into_body();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(hito_server::serve(
        listener,
        f.app.clone(),
        f.services.clone(),
        async {
            let _ = stopped.await;
        },
    ));
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\n\r\n",
                f.token
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut headers = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(socket.read_u8().await.unwrap());
            assert!(headers.len() <= 16_384);
        }
        socket.read_u8().await.unwrap();
    })
    .await
    .unwrap();
    let tcp_started = headers.starts_with(b"HTTP/1.1 200 OK\r\n");
    stop.send(()).unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(8), serving)
        .await
        .unwrap()
        .unwrap()
        .is_ok();
    let retained_denied = matches!(retained_body.frame().await, Some(Err(_)));
    let new_denied = f.begin().await.status() == StatusCode::CONFLICT;
    let listener_closed = TcpStream::connect(address).await.is_err();
    drop(retained_body);
    drop(socket);
    f.close().await;
    assert!(
        tcp_started && joined && retained_denied && new_denied && listener_closed,
        "TCP shutdown must close the playback capability before fixture cleanup: tcp={tcp_started}, joined={joined}, retained_denied={retained_denied}, new_denied={new_denied}, listener_closed={listener_closed}"
    );
}
