use tokio::{io::AsyncReadExt, net::TcpStream};
use tokio_rustls::server::TlsStream;

pub struct Request {
    pub server_name: String,
    pub method: String,
    pub target: String,
    headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub(super) async fn read_request(socket: &mut TlsStream<TcpStream>) -> Request {
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
    assert!(line.len() == 3 && line[0] == "GET" && line[2] == "HTTP/1.1");
    let method = line[0].to_owned();
    let target = line[1].to_owned();
    let headers: Vec<_> = lines
        .map(|line| {
            let (name, value) = line
                .split_once(':')
                .expect("the owned request header must have a value");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    assert!(headers.len() <= 32 && !headers.iter().any(|(name, _)| name == "transfer-encoding"));
    let lengths: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name == "content-length")
        .collect();
    assert!(lengths.is_empty() || (lengths.len() == 1 && lengths[0].1 == "0"));
    Request {
        server_name,
        method,
        target,
        headers,
    }
}
