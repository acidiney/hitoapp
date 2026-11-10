use super::{Endpoint, Remote, catalog::Call, raw::RawServer, transport::RemoteError};
use std::time::{Duration, Instant};

#[tokio::test]
async fn catalog_header_and_whole_json_body_timeouts_close_owned_connections() {
    for call in Call::ALL {
        let mut responses = vec![Vec::new()];
        if !matches!(call, Call::Release) {
            responses.push(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 999\r\n\r\n{".to_vec());
        }
        for response in responses {
            let server = RawServer::start(response, true).await;
            let remote = Remote::with_deadlines(
                Endpoint::parse(&server.endpoint).unwrap(),
                Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .unwrap();
            let started = Instant::now();
            assert_eq!(
                call.run(&remote).await.unwrap_err(),
                RemoteError::Unavailable
            );
            assert!(started.elapsed() < Duration::from_secs(1));
            server.close().await;
        }
    }
}

#[tokio::test]
async fn cancelling_any_catalog_request_closes_its_owned_connection() {
    for call in Call::ALL {
        let server = RawServer::start(Vec::new(), true).await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        let task = tokio::spawn(async move { call.run(&remote).await });
        server.received().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        server.close().await;
    }
}

#[tokio::test]
async fn cancelling_a_partial_catalog_json_body_closes_the_connection() {
    for call in [Call::Begin, Call::Page] {
        let server = RawServer::start(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 999\r\n\r\n{"
                .to_vec(),
            true,
        )
        .await;
        let remote = Remote::new(Endpoint::parse(&server.endpoint).unwrap()).unwrap();
        let task = tokio::spawn(async move { call.run(&remote).await });
        server.responded().await;
        tokio::task::yield_now().await;
        assert!(
            !task.is_finished(),
            "incomplete body unexpectedly completed"
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        server.close().await;
    }
}

#[tokio::test]
async fn catalog_body_deadline_is_not_reset_by_arriving_chunks() {
    use super::fixture::{Reply, Server};
    for call in [Call::Begin, Call::Page] {
        let mut reply = Reply::json(serde_json::json!({}));
        reply.chunks = vec![(Duration::from_millis(40), b" ".to_vec()); 10];
        let server = Server::scripted(vec![reply]).await;
        let remote = Remote::with_deadlines(
            Endpoint::parse(&server.endpoint).unwrap(),
            Duration::from_secs(1),
            Duration::from_millis(100),
        )
        .unwrap();
        let started = Instant::now();
        assert_eq!(
            call.run(&remote).await.unwrap_err(),
            RemoteError::Unavailable
        );
        assert!(started.elapsed() < Duration::from_millis(350));
        server.close().await;
    }
}
