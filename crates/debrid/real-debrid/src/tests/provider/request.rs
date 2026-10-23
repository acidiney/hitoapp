use std::time::Instant;
use tokio::{io::AsyncReadExt, net::TcpStream};

const MAX_HEADER_BYTES: usize = 8192;
const MAX_HEADERS: usize = 32;
const MAX_BODY_BYTES: usize = 65_536;

pub(in crate::tests) struct ObservedRequest {
    pub method: String,
    pub target: String,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub buffered_body_bytes: usize,
    pub received_at: Instant,
}

impl ObservedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub(in crate::tests) async fn read_request(socket: &mut TcpStream) -> ObservedRequest {
    let mut frame = Vec::new();
    let header_end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        let remaining = MAX_HEADER_BYTES.saturating_sub(frame.len());
        assert!(
            remaining > 0,
            "provider request headers exceeded their limit"
        );
        let mut chunk = [0_u8; 1024];
        let capacity = remaining.min(chunk.len());
        let length = socket
            .read(&mut chunk[..capacity])
            .await
            .expect("the fixture request must be readable");
        assert!(length > 0, "the fixture request ended before its headers");
        frame.extend_from_slice(&chunk[..length]);
    };
    let headers =
        std::str::from_utf8(&frame[..header_end]).expect("the owned request headers must be UTF-8");
    let received_at = Instant::now();
    let mut lines = headers.split("\r\n");
    let mut request_line = lines
        .next()
        .expect("the request line must exist")
        .split(' ');
    let method = request_line
        .next()
        .expect("the method must exist")
        .to_owned();
    let target = request_line
        .next()
        .expect("the target must exist")
        .to_owned();
    assert!(
        request_line.next() == Some("HTTP/1.1"),
        "the owned request must use HTTP/1.1"
    );
    assert!(request_line.next().is_none());
    let headers: Vec<(String, String)> = lines
        .map(|line| {
            let (name, value) = line
                .split_once(':')
                .expect("the request header must have a value");
            assert!(
                !name.is_empty()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    }),
                "the request header name must be valid"
            );
            assert!(
                !value
                    .bytes()
                    .any(|byte| byte.is_ascii_control() && byte != b'\t'),
                "the request header value must be valid"
            );
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    assert!(
        headers.len() <= MAX_HEADERS,
        "provider request header count exceeded its limit"
    );
    assert!(
        !headers.iter().any(|(name, _)| name == "transfer-encoding"),
        "the owned fixture requires fixed-length request framing"
    );
    let lengths: Vec<&str> = headers
        .iter()
        .filter_map(|(name, value)| (name == "content-length").then_some(value.as_str()))
        .collect();
    assert!(
        lengths.len() <= 1,
        "the request body length must be unambiguous"
    );
    let body_length = lengths.first().map_or(0, |length| {
        assert!(
            !length.is_empty() && length.bytes().all(|byte| byte.is_ascii_digit()),
            "the request body length must be a decimal integer"
        );
        length
            .parse::<usize>()
            .expect("the request body length must fit its bound")
    });
    assert!(
        body_length <= MAX_BODY_BYTES,
        "provider request body exceeded its limit"
    );
    let mut body = frame[header_end + 4..].to_vec();
    let buffered_body_bytes = body.len();
    assert!(
        buffered_body_bytes <= body_length,
        "the captured request must not exceed its declared body length"
    );
    while body.len() < body_length {
        let mut chunk = [0_u8; 1024];
        let capacity = (body_length - body.len()).min(chunk.len());
        let length = socket
            .read(&mut chunk[..capacity])
            .await
            .expect("the fixture request body must be readable");
        assert!(
            length > 0,
            "the fixture request ended before its declared body"
        );
        body.extend_from_slice(&chunk[..length]);
    }
    ObservedRequest {
        method,
        target,
        headers,
        body,
        buffered_body_bytes,
        received_at,
    }
}
