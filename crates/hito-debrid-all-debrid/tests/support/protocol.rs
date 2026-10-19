use super::Request;
use std::time::Instant;
use tokio::{io::AsyncReadExt, net::TcpStream};
use tokio_rustls::server::TlsStream;

pub(crate) async fn read_request(
    socket: &mut TlsStream<TcpStream>,
    expected_method: &str,
) -> Request {
    let server_name = socket
        .get_ref()
        .1
        .server_name()
        .expect("the owned TLS request must have SNI")
        .to_owned();
    let mut frame = Vec::new();
    let end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        assert!(
            frame.len() < 8192,
            "the owned request headers exceeded their bound"
        );
        let mut byte = [0_u8; 1];
        socket
            .read_exact(&mut byte)
            .await
            .expect("the owned request headers must complete");
        frame.push(byte[0]);
    };
    let head = std::str::from_utf8(&frame[..end]).expect("the owned request headers must be UTF-8");
    let mut lines = head.split("\r\n");
    let line: Vec<_> = lines
        .next()
        .expect("the owned request line must exist")
        .split(' ')
        .collect();
    assert!(
        line.len() == 3 && line[2] == "HTTP/1.1",
        "the owned request framing changed"
    );
    let method = line[0].to_owned();
    assert!(matches!(expected_method, "GET" | "POST") && method == expected_method);
    let target = line[1].to_owned();
    let headers: Vec<_> = lines
        .map(|line| {
            let (key, value) = line
                .split_once(':')
                .expect("the owned request header must have a value");
            (key.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    assert!(headers.len() <= 32 && !headers.iter().any(|(name, _)| name == "transfer-encoding"));
    let lengths: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name == "content-length")
        .collect();
    let length = if expected_method == "GET" {
        assert!(lengths.is_empty() || (lengths.len() == 1 && lengths[0].1 == "0"));
        0
    } else {
        assert!(
            lengths.len() == 1,
            "the owned request body length must be unambiguous"
        );
        lengths[0]
            .1
            .parse::<usize>()
            .expect("the owned request body length must be numeric")
    };
    assert!(length <= 4096, "the owned request body exceeded its bound");
    let mut body = vec![0; length];
    socket
        .read_exact(&mut body)
        .await
        .expect("the owned request body must complete");
    let received_at = Instant::now();
    Request {
        received_at,
        server_name,
        method,
        target,
        body,
        headers,
    }
}
