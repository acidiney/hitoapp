use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};
use uuid::Uuid;

pub struct Reply {
    pub status: u16,
    pub request_id: Uuid,
    pub body: Vec<u8>,
}

pub struct Receipt {
    pub operation: &'static str,
    pub status: u16,
    pub request_id: Uuid,
}

impl Reply {
    pub fn receipt(&self, operation: &'static str) -> Receipt {
        Receipt {
            operation,
            status: self.status,
            request_id: self.request_id,
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
    write!(socket, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Plugin-Protocol: 1\r\nCookie: owned-private-premiumize-runtime-cookie\r\n")
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
    let ids = values("x-request-id");
    assert_eq!(ids.len(), 1);
    let request_id = Uuid::parse_str(ids[0])
        .unwrap_or_else(|_| panic!("owned HTTP requests must have a safe correlation UUID"));
    assert_eq!(request_id.get_version_num(), 4);
    Reply {
        status,
        request_id,
        body: bytes[end + 4..].to_vec(),
    }
}
