use super::*;
use crate::Error;
use futures_util::FutureExt;
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr},
    panic::AssertUnwindSafe,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

const BODY: &[u8] = b"<rss version=\"2.0\"><channel><title>Owned</title></channel></rss>";

#[tokio::test]
async fn pinned_trusted_feed_cannot_redirect_into_an_untrusted_origin_or_use_mixed_dns() {
    let blocked = Server::start(None).await;
    let allowed = Server::start(Some(format!(
        "http://127.0.0.1:{}/blocked",
        blocked.address.port()
    )))
    .await;
    let result = AssertUnwindSafe(async {
        let origin = format!("http://owned.invalid:{}", allowed.address.port());
        let config = PodcastNetworkConfig::new(vec![origin.clone()])
            .unwrap_or_else(|_| panic!("the owned administrator origin must be accepted"));
        let resolver = Arc::new(FixedResolver { addresses: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)], calls: AtomicUsize::new(0) });
        let remote = RemoteHttp::with_resolver(&config, resolver.clone());
        let fetched = remote.get(&format!("{origin}/feed"), ConditionalValidators::default()).await
            .unwrap_or_else(|_| panic!("the explicitly trusted feed must reach its pinned owned TCP destination"));
        assert!(fetched.response.status() == reqwest::StatusCode::OK &&
            fetched.final_url.as_str() == format!("{origin}/feed") && !fetched.permanent_redirect,
            "a pinned request must preserve its original host and truthful final address");
        let body = fetched.response.bytes().await
            .unwrap_or_else(|_| panic!("the owned response body must remain readable"));
        assert!(body.as_ref() == BODY, "the request must receive the exact owned TCP response");
        assert!(resolver.calls.load(Ordering::SeqCst) == 1,
            "the connection must use the validated DNS snapshot without a second lookup");
        {
            let seen = allowed.requests.lock().unwrap();
            assert!(seen.len() == 1 && seen[0].target == "/feed" && seen[0].method == "GET" &&
                seen[0].headers.get("host") == Some(&format!("owned.invalid:{}", allowed.address.port())) &&
                seen[0].headers.get("accept-encoding").map(String::as_str) == Some("identity") &&
                seen[0].headers.get("connection").map(String::as_str) == Some("close") &&
                seen[0].headers.get("accept").is_some_and(|value| value.contains("application/rss+xml")) &&
                ["cookie", "authorization", "proxy-authorization"].iter().all(|key| !seen[0].headers.contains_key(*key)),
                "the actual connection must preserve Host and use only the fixed feed transport headers");
        }
        let redirected = remote.get(&format!("{origin}/redirect"), ConditionalValidators::default()).await;
        assert!(matches!(redirected, Err(Error::Invalid(_))),
            "every redirect must reject an untrusted private destination before connecting");
        assert!(allowed.requests.lock().unwrap().len() == 2 && blocked.requests.lock().unwrap().is_empty(),
            "the redirect must be received without reaching the forbidden destination");
        let mixed = RemoteHttp::with_resolver(&PodcastNetworkConfig::default(), Arc::new(FixedResolver {
            addresses: vec![IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))],
            calls: AtomicUsize::new(0),
        }));
        let result = mixed.get(&format!("http://mixed.invalid:{}/feed", allowed.address.port()), ConditionalValidators::default()).await;
        assert!(matches!(result, Err(Error::Invalid(_))),
            "mixed public and private DNS answers must be rejected as one snapshot");
        assert!(allowed.requests.lock().unwrap().len() == 2 && blocked.requests.lock().unwrap().is_empty(),
            "mixed DNS rejection must perform no connection to either owned destination");
    }).catch_unwind();
    let result = tokio::time::timeout(Duration::from_secs(8), result).await;
    let allowed_closed = allowed.close().await;
    let blocked_closed = blocked.close().await;
    assert!(
        allowed_closed && blocked_closed,
        "every owned TCP server task must be joined without hiding a fixture failure"
    );
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned remote HTTP boundary exercise must finish within its deadline"),
    }
}

#[tokio::test]
async fn empty_userinfo_is_rejected_before_connecting_to_an_explicitly_trusted_origin() {
    let server = Server::start(None).await;
    let origin = format!("http://127.0.0.1:{}", server.address.port());
    let config = PodcastNetworkConfig::new(vec![origin])
        .unwrap_or_else(|_| panic!("the plain owned administrator origin must be accepted"));
    let remote = RemoteHttp::new(&config);
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        remote.get(
            &format!("http://@127.0.0.1:{}/feed", server.address.port()),
            ConditionalValidators::default(),
        ),
    )
    .await;
    let requests = server.requests.lock().unwrap().len();
    let closed = server.close().await;
    assert!(
        closed,
        "the owned userinfo test server must shut down and join"
    );
    let result =
        result.unwrap_or_else(|_| panic!("userinfo rejection must finish within its deadline"));
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "empty userinfo syntax must be rejected before URL normalization removes it"
    );
    assert!(
        requests == 0,
        "a rejected userinfo URL must never reach an explicitly trusted TCP destination"
    );
}

struct FixedResolver {
    addresses: Vec<IpAddr>,
    calls: AtomicUsize,
}

impl Resolver for FixedResolver {
    fn resolve<'a>(&'a self, _host: &'a str) -> Lookup<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.addresses.clone()) })
    }
}

struct Seen {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
}
struct Server {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<Seen>>>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(redirect: Option<String>) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0; 1];
                    let count =
                        tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
                            .await
                            .unwrap()
                            .unwrap();
                    assert!(
                        count == 1 && bytes.len() < 8192,
                        "the owned request headers must be bounded"
                    );
                    bytes.extend_from_slice(&byte);
                }
                let text = std::str::from_utf8(&bytes).unwrap();
                let mut lines = text.split("\r\n");
                let mut request = lines.next().unwrap().split_whitespace();
                let method = request.next().unwrap().to_owned();
                let target = request.next().unwrap().to_owned();
                let headers = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                    .collect();
                let redirecting = target == "/redirect";
                received.lock().unwrap().push(Seen {
                    method,
                    target,
                    headers,
                });
                let response = if redirecting {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        redirect.as_ref().unwrap()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        BODY.len(),
                        std::str::from_utf8(BODY).unwrap()
                    )
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            address,
            requests,
            task,
        }
    }

    async fn close(mut self) -> bool {
        self.task.abort();
        let result = (&mut self.task).await;
        result.is_ok() || result.is_err_and(|error| error.is_cancelled())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
