use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_HEADER_BYTES: usize = 8192;
const MAX_HEADERS: usize = 32;
const MAX_REQUEST_BODY_BYTES: usize = 65_536;

pub(super) struct Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
}

pub(super) async fn read_request(socket: &mut (impl AsyncRead + Unpin)) -> Request {
    let mut frame = Vec::new();
    let header_end = loop {
        if let Some(offset) = frame.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break offset;
        }
        let remaining = MAX_HEADER_BYTES.saturating_sub(frame.len());
        assert!(
            remaining > 0,
            "the owned TLS request headers exceeded their bound"
        );
        let mut chunk = [0_u8; 1024];
        let capacity = remaining.min(chunk.len());
        let length = socket
            .read(&mut chunk[..capacity])
            .await
            .expect("the owned TLS request must be readable");
        assert!(length > 0, "the owned TLS request ended before its headers");
        frame.extend_from_slice(&chunk[..length]);
    };
    let header = std::str::from_utf8(&frame[..header_end])
        .expect("the owned TLS request headers must be UTF-8");
    let mut lines = header.split("\r\n");
    let mut request_line = lines
        .next()
        .expect("the owned request line must exist")
        .split(' ');
    let method = request_line
        .next()
        .expect("the owned method must exist")
        .to_owned();
    let target = request_line
        .next()
        .expect("the owned target must exist")
        .to_owned();
    assert!(
        request_line.next() == Some("HTTP/1.1"),
        "the owned request must use HTTP/1.1"
    );
    assert!(
        request_line.next().is_none(),
        "the owned request line must be bounded"
    );
    let headers: Vec<(String, String)> = lines
        .map(|line| {
            let (name, value) = line
                .split_once(':')
                .expect("the owned header must have a value");
            assert!(
                !name.is_empty()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    }),
                "the owned header name must be valid"
            );
            assert!(
                !value
                    .bytes()
                    .any(|byte| byte.is_ascii_control() && byte != b'\t'),
                "the owned header value must be valid"
            );
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    assert!(
        headers.len() <= MAX_HEADERS,
        "the owned TLS request header count exceeded its bound"
    );
    assert!(
        !headers.iter().any(|(name, _)| name == "transfer-encoding"),
        "owned request framing must have a fixed length"
    );
    let lengths: Vec<&str> = headers
        .iter()
        .filter_map(|(name, value)| (name == "content-length").then_some(value.as_str()))
        .collect();
    assert!(
        lengths.len() <= 1,
        "the owned request body length must be unambiguous"
    );
    let length = lengths.first().map_or(0, |length| {
        assert!(
            !length.is_empty() && length.bytes().all(|byte| byte.is_ascii_digit()),
            "the owned body length must be decimal"
        );
        length
            .parse::<usize>()
            .expect("the owned body length must fit its bound")
    });
    assert!(
        length <= MAX_REQUEST_BODY_BYTES,
        "the owned TLS request body exceeded its bound"
    );
    let mut body = frame[header_end + 4..].to_vec();
    assert!(
        body.len() <= length,
        "the owned request exceeded its declared body length"
    );
    while body.len() < length {
        let mut chunk = [0_u8; 1024];
        let capacity = (length - body.len()).min(chunk.len());
        let count = socket
            .read(&mut chunk[..capacity])
            .await
            .expect("the owned TLS request body must be readable");
        assert!(
            count > 0,
            "the owned request ended before its declared body"
        );
        body.extend_from_slice(&chunk[..count]);
    }
    Request {
        method,
        target,
        headers,
        body,
    }
}

pub(super) enum Reply<'a> {
    Json(&'a [u8]),
    Wav(&'a [u8]),
    Rejected,
}

pub(super) struct FramedReply {
    pub headers: Vec<u8>,
    pub body: Vec<u8>,
}

impl FramedReply {
    pub fn validate(&self) {
        assert!(self.headers.len() <= MAX_HEADER_BYTES && self.headers.ends_with(b"\r\n\r\n"));
        assert!(
            self.headers
                .windows(2)
                .filter(|bytes| *bytes == b"\r\n")
                .count()
                <= MAX_HEADERS + 3
        );
        assert!(self.body.len() <= 1_048_576);
    }
}

pub(super) async fn framed_reply(socket: &mut (impl AsyncWrite + Unpin), response: &FramedReply) {
    response.validate();
    let result = async {
        socket.write_all(&response.headers).await?;
        socket.write_all(&response.body).await?;
        socket.shutdown().await
    }
    .await;
    if let Err(error) = result {
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::UnexpectedEof
            ),
            "the rejected owned TLS response had an unexpected write failure"
        );
    }
}

pub(super) async fn reply(socket: &mut (impl AsyncWrite + Unpin), reply: Reply<'_>) {
    let (status, kind, body, media) = match reply {
        Reply::Json(body) => ("200 OK", "application/json", body, false),
        Reply::Wav(body) => ("200 OK", "audio/wav", body, true),
        Reply::Rejected => (
            "500 Internal Server Error",
            "application/json",
            b"{}".as_slice(),
            false,
        ),
    };
    assert!(body.len() <= if media { 1_048_576 } else { 65_536 });
    let extra = if media {
        "Accept-Ranges: bytes\r\nETag: \"owned-pcm16-fixture\"\r\nContent-Disposition: attachment; filename=\"private-media-name-canary.wav\"\r\nSet-Cookie: private-media-cookie-canary=private-media-cookie-value\r\n"
    } else {
        "Set-Cookie: private-api-cookie-canary=private-api-cookie-value\r\n"
    };
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    ).into_bytes();
    response.extend_from_slice(body);
    socket
        .write_all(&response)
        .await
        .expect("the owned TLS reply must be sent");
    socket
        .shutdown()
        .await
        .expect("the owned TLS reply must close");
}
