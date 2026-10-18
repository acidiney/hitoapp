use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use serde_json::Value;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub async fn begin(
    address: SocketAddr,
    path: &str,
    body: &Value,
    mut headers: HeaderMap,
) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(3), async {
        let body = serde_json::to_vec(body).unwrap();
        assert!(body.len() <= 262_144);
        headers.insert("origin", format!("http://{address}").parse().unwrap());
        let mut head = format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in &headers {
            head.push_str(name.as_str());
            head.push_str(": ");
            head.push_str(value.to_str().unwrap());
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
        socket
    })
    .await
    .expect("The owned artist search must reach the hosted HTTP connection")
}

pub async fn finish(mut socket: TcpStream) -> (StatusCode, HeaderMap, Value) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).await.unwrap();
            if count == 0 {
                break;
            }
            assert!(count <= 262_144_usize.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk[..count]);
        }
        let split = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        assert!(split <= 16_384);
        let mut lines = std::str::from_utf8(&bytes[..split]).unwrap().split("\r\n");
        let status = StatusCode::from_u16(
            lines
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap(),
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        for line in lines {
            let (name, value) = line.split_once(':').unwrap();
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value.trim()).unwrap(),
            );
        }
        assert!(headers.get("transfer-encoding").is_none());
        let body = &bytes[split + 4..];
        assert_eq!(
            headers["content-length"]
                .to_str()
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            body.len()
        );
        (status, headers, serde_json::from_slice(body).unwrap())
    })
    .await
    .expect("The hosted artist search retry must finish promptly")
}
