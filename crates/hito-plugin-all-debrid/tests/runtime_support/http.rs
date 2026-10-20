use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

pub fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    credential: Option<&str>,
    body: &str,
) -> (u16, Vec<u8>) {
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    write!(socket,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Plugin-Protocol: 1\r\nCookie: owned-private-bootstrap-cookie\r\n").unwrap();
    if let Some(credential) = credential {
        write!(socket, "Authorization: Bearer {credential}\r\n").unwrap();
    }
    write!(socket, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut bytes = Vec::new();
    socket.take(73_729).read_to_end(&mut bytes).unwrap();
    assert!(
        bytes.len() <= 73_728,
        "owned runtime response must remain bounded"
    );
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    assert!(end <= 8192);
    let head = std::str::from_utf8(&bytes[..end]).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let fields = lines
        .map(|line| line.split_once(':').unwrap())
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim()))
        .collect::<Vec<_>>();
    assert!(fields.len() <= 32);
    assert!(!fields.iter().any(|(name, _)| name == "transfer-encoding"));
    let values = |name: &str| {
        fields
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| *value)
            .collect::<Vec<_>>()
    };
    assert_eq!(values("cache-control"), ["private, no-store"]);
    assert_eq!(values("x-content-type-options"), ["nosniff"]);
    assert_eq!(values("content-type"), ["application/json"]);
    let lengths = values("content-length");
    assert_eq!(lengths.len(), 1);
    assert_eq!(lengths[0].parse::<usize>().unwrap(), bytes.len() - end - 4);
    (status, bytes[end + 4..].to_vec())
}
