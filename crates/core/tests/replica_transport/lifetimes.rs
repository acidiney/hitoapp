use super::{Endpoint, Remote, TOKEN, raw::RawServer, transport::RemoteError};
use std::time::{Duration, Instant};

#[tokio::test]
async fn header_and_whole_body_deadlines_close_the_connection() {
    for response in [
        Vec::new(),
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 999\r\n\r\n{"
            .to_vec(),
    ] {
        let server = RawServer::start(response, true).await;
        let remote = Remote::with_deadlines(
            Endpoint::parse(&server.endpoint).unwrap(),
            Duration::from_millis(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let started = Instant::now();
        assert_eq!(
            remote.me(TOKEN, "owned-account").await.unwrap_err(),
            RemoteError::Unavailable
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        server.close().await;
    }
}

#[tokio::test]
async fn truncated_body_is_unavailable_without_waiting_for_the_body_deadline() {
    let server = RawServer::start(
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 999\r\n\r\n{"
            .to_vec(),
        false,
    )
    .await;
    let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
    let started = Instant::now();
    assert_eq!(
        remote.me(TOKEN, "owned-account").await.unwrap_err(),
        RemoteError::Unavailable
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    server.close().await;
}
