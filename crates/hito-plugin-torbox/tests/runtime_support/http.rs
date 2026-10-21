use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

pub struct Receipt {
    pub operation: &'static str,
    pub status: u16,
}

impl Reply {
    pub fn receipt(&self, operation: &'static str) -> Receipt {
        Receipt {
            operation,
            status: self.status,
        }
    }
}

pub fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    credential: Option<&str>,
    body: &str,
) -> Reply {
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .unwrap_or_else(|_| panic!("the owned runtime must accept HTTP"));
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap_or_else(|_| panic!("owned HTTP reads must be bounded"));
    socket
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap_or_else(|_| panic!("owned HTTP writes must be bounded"));
    write!(socket, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Plugin-Protocol: 1\r\nCookie: owned-private-torbox-runtime-cookie\r\n")
        .ok().expect("owned HTTP headers must write");
    if let Some(credential) = credential {
        write!(socket, "Authorization: Bearer {credential}\r\n")
            .ok()
            .expect("owned authentication must write");
    }
    write!(socket, "Content-Length: {}\r\n\r\n{body}", body.len())
        .ok()
        .expect("the owned HTTP body must write");
    let mut bytes = Vec::new();
    socket
        .take(73_729)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|_| panic!("the owned HTTP response must complete"));
    assert!(
        bytes.len() <= 73_728,
        "owned HTTP responses must stay bounded"
    );
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("owned HTTP headers must terminate");
    assert!(end <= 8192, "owned response headers must stay bounded");
    let head = std::str::from_utf8(&bytes[..end])
        .unwrap_or_else(|_| panic!("owned HTTP headers must be UTF-8"));
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .expect("owned HTTP must include a status")
        .split_whitespace()
        .nth(1)
        .expect("owned HTTP must include a status code")
        .parse::<u16>()
        .unwrap_or_else(|_| panic!("owned HTTP status must be numeric"));
    let fields = lines
        .map(|line| {
            line.split_once(':')
                .expect("owned response headers must be fields")
        })
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim()))
        .collect::<Vec<_>>();
    assert!(
        fields.len() <= 32 && !fields.iter().any(|(name, _)| name == "transfer-encoding"),
        "owned JSON responses must use bounded unambiguous headers"
    );
    let values = |name: &str| {
        fields
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| *value)
            .collect::<Vec<_>>()
    };
    assert!(values("cache-control") == ["private, no-store"]);
    assert!(values("x-content-type-options") == ["nosniff"]);
    assert!(values("content-type") == ["application/json"]);
    let lengths = values("content-length");
    assert_eq!(lengths.len(), 1);
    assert!(
        lengths[0].parse::<usize>().ok() == Some(bytes.len() - end - 4),
        "owned JSON length must be exact"
    );
    Reply {
        status,
        body: bytes[end + 4..].to_vec(),
    }
}

pub fn hold_search_body(address: SocketAddr, credential: &str) -> TcpStream {
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .expect("the owned active HTTP connection must open");
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("the active HTTP witness must remain bounded");
    socket
        .set_write_timeout(Some(Duration::from_secs(1)))
        .expect("active HTTP headers must remain bounded");
    write!(socket, "POST /v1/search HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Plugin-Protocol: 1\r\nAuthorization: Bearer {credential}\r\nCookie: owned-private-torbox-runtime-held-cookie\r\nContent-Length: 64\r\nExpect: 100-continue\r\n\r\n")
        .expect("the owned active HTTP headers must write");
    let expected = b"HTTP/1.1 100 Continue\r\n\r\n";
    let mut received = vec![0; expected.len()];
    socket
        .read_exact(&mut received)
        .expect("the handler must poll its held request body");
    assert!(
        received == expected,
        "the active request must have a real HTTP body-poll witness"
    );
    socket
}

pub fn assert_closed(socket: &mut TcpStream) {
    let mut byte = [0_u8; 1];
    match socket.read(&mut byte) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::BrokenPipe
            ) => {}
        _ => panic!("the active HTTP peer must close after joined shutdown"),
    }
}
