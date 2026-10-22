use hito_testkit::tls;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

pub async fn listener(host: &str) -> (TcpListener, TlsAcceptor, Vec<u8>) {
    let tls::OwnedTls { acceptor, root_der } = tls::issue(host);
    (
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
        acceptor,
        root_der,
    )
}

pub async fn request(socket: &mut TlsStream<TcpStream>, host: &str) -> String {
    assert!(socket.get_ref().1.server_name() == Some(host));
    let mut bytes = Vec::new();
    let end = loop {
        if let Some(i) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
            break i;
        }
        assert!(bytes.len() < 8192);
        let mut byte = [0];
        socket.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
    };
    let mut lines = std::str::from_utf8(&bytes[..end]).unwrap().split("\r\n");
    let line: Vec<_> = lines.next().unwrap().split(' ').collect();
    assert!(line.len() == 3 && line[0] == "GET" && line[2] == "HTTP/1.1");
    let headers: Vec<_> = lines
        .map(|x| {
            let (k, v) = x.split_once(':').unwrap();
            (k.to_ascii_lowercase(), v.trim())
        })
        .collect();
    assert!(headers.len() <= 32);
    assert!(headers.iter().any(|(k, v)| k == "host" && *v == host));
    for (key, value) in &headers {
        assert!(key != "transfer-encoding" && (key != "content-length" || *value == "0"));
        if host == "media.hito-fixture.invalid" {
            assert!(
                !["authorization", "cookie", "referer", "proxy-authorization"]
                    .contains(&key.as_str())
            );
        }
    }
    line[1].into()
}

pub async fn peer_closed(socket: &mut TlsStream<TcpStream>) {
    let mut byte = [0];
    match socket.read(&mut byte).await {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NotConnected
            ) => {}
        _ => panic!("owned peer did not close its TLS transport"),
    }
}
