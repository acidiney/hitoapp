use super::fixture::{Fixture, ROOT, host, tag_wav};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::support::write_wav;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::test]
async fn a_committed_response_disconnect_resumes_the_same_snapshot_over_owned_tcp() {
    let f = Fixture::start().await;
    let path = f.music.join("Owned.wav");
    write_wav(&path, 0);
    tag_wav(&path, "Before the lost acknowledgement");
    f.scan().await;
    let before = f
        .call("GET", "/api/v1/sync?cursor=0", Value::Null)
        .await
        .json();
    let mut server = host::Host::start(f.services.clone()).await;
    let outcome = tokio::time::timeout(Duration::from_secs(15), AssertUnwindSafe(async {
        let body = json!({"library_id":f.library}).to_string();
        let mut socket = TcpStream::connect(server.address).await.unwrap();
        socket.write_all(format!("POST {ROOT} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: Bearer {}\r\nX-Hito-Request: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", server.address, f.token, body.len()).as_bytes()).await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(socket.read_u8().await.unwrap());
            assert!(head.len() <= 16_384);
        }
        let head = std::str::from_utf8(&head).unwrap();
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("cache-control: private, no-store\r\n"));
        assert!(!head.contains("transfer-encoding:"));
        let mut prefix = vec![0; b"{\"id\":\"".len() + 36 + 1];
        socket.read_exact(&mut prefix).await.unwrap();
        let prefix = std::str::from_utf8(&prefix).unwrap();
        let id = prefix.strip_prefix("{\"id\":\"").unwrap().strip_suffix('"').unwrap();
        let committed_id = uuid::Uuid::parse_str(id).unwrap();
        drop(socket);
        tag_wav(&path, "Changed after the lost acknowledgement");
        f.scan().await;
        let retry = f.begin().await;
        assert_eq!(retry["id"], committed_id.to_string());
        let page_path = format!("{ROOT}/{committed_id}/pages/0");
        let page = f.call("GET", &page_path, Value::Null).await;
        assert_eq!(page.status, StatusCode::OK);
        assert_eq!(page.json()["recordings"][0]["title"], "Before the lost acknowledgement");
        assert_eq!(f.begin().await, retry);
        assert_eq!(f.call("GET", &page_path, Value::Null).await.bytes, page.bytes);
        assert_eq!(f.call("GET", "/api/v1/sync?cursor=0", Value::Null).await.json(), before);
    }).catch_unwind()).await;
    server.request_shutdown();
    let joined = server.join().await;
    assert!(
        joined,
        "The owned Hito server must close and join all workers"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => {
            panic!("The committed-response disconnect workflow must finish within its deadline")
        }
    }
}
