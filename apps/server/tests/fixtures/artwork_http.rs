use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use serde_json::Value;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

pub async fn call(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> Response {
    tokio::time::timeout(Duration::from_secs(3), async {
        let body = body.map(|body| body.to_string()).unwrap_or_default();
        let authorization = token
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Request: 1\r\n{authorization}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let split = bytes.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
        let mut lines = std::str::from_utf8(&bytes[..split]).unwrap().split("\r\n");
        let status = StatusCode::from_u16(
            lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap(),
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
        let body = bytes[split + 4..].to_vec();
        assert!(headers.get("transfer-encoding").is_none());
        if status == StatusCode::NO_CONTENT {
            assert!(body.is_empty());
            assert!(headers.get("content-length").is_none());
        } else {
            assert_eq!(
                headers["content-length"].to_str().unwrap().parse::<usize>().unwrap(),
                body.len()
            );
        }
        Response { status, headers, body }
    })
    .await
    .expect("the owned HTTP request must finish")
}

pub async fn get(address: SocketAddr, token: &str, path: &str) -> Value {
    let response = call(address, "GET", path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    response.json()
}
