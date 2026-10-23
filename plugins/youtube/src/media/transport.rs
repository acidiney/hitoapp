mod body;
pub(super) use body::Body;

use super::range::Selection;
use crate::Error;
use axum::{
    body::Bytes,
    http::{Method, Request, Response},
};
use http_body_util::Empty;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use rustls::{ClientConfig, pki_types::ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use std::{
    future::{Future, poll_fn},
    sync::Arc,
    task::Poll,
    time::Duration,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use url::Url;

#[derive(Clone)]
pub(crate) struct Client {
    tls: Arc<ClientConfig>,
    #[cfg(test)]
    endpoint: Option<(String, std::net::SocketAddr)>,
}

impl Client {
    pub(crate) fn new() -> Result<Self, Error> {
        let tls =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|_| Error::Unavailable)?
                .with_platform_verifier()
                .map_err(|_| Error::Unavailable)?
                .with_no_client_auth();
        Ok(Self::configured(tls))
    }

    fn configured(mut tls: ClientConfig) -> Self {
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        Self {
            tls: Arc::new(tls),
            #[cfg(test)]
            endpoint: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_fixture(
        tls: ClientConfig,
        host: &str,
        address: std::net::SocketAddr,
    ) -> Self {
        let mut client = Self::configured(tls);
        client.endpoint = Some((host.to_owned(), address));
        client
    }

    pub(super) async fn request(
        &self,
        url: &Url,
        method: Method,
        selected: Option<Selection>,
    ) -> Result<Response<Body>, Error> {
        let host = url.host_str().ok_or(Error::InvalidMedia)?;
        let name = ServerName::try_from(host.to_owned()).map_err(|_| Error::InvalidMedia)?;
        let connecting = async {
            let socket = self.connect(host).await?;
            TlsConnector::from(self.tls.clone())
                .connect(name, socket)
                .await
                .map_err(|_| Error::Unavailable)
        };
        let tls = tokio::time::timeout(Duration::from_secs(2), connecting)
            .await
            .map_err(|_| Error::Unavailable)??;
        let (mut sender, connection) = http1::Builder::new()
            .max_headers(32)
            .max_header_size(16_384)
            .max_buf_size(262_144)
            .handshake(TokioIo::new(tls))
            .await
            .map_err(|_| Error::Unavailable)?;
        let target = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_owned(),
        };
        let mut request = Request::builder()
            .method(method)
            .uri(target)
            .header("host", host)
            .header("accept", "audio/mp4")
            .header("accept-encoding", "identity")
            .header("connection", "close")
            .header("user-agent", concat!("Hito/", env!("CARGO_PKG_VERSION")));
        if let Some(selected) = selected {
            request = request.header("range", selected.header());
        }
        let request = request
            .body(Empty::<Bytes>::new())
            .map_err(|_| Error::Unavailable)?;
        let mut sending = Box::pin(sender.send_request(request));
        let mut driver = Some(Box::pin(connection));
        let response = poll_fn(|context| {
            if let Some(current) = driver.as_mut() {
                match current.as_mut().poll(context) {
                    Poll::Ready(Ok(())) => driver = None,
                    Poll::Ready(Err(_)) => return Poll::Ready(Err(Error::Unavailable)),
                    Poll::Pending => {}
                }
            }
            match sending.as_mut().poll(context) {
                Poll::Ready(result) => Poll::Ready(result.map_err(|_| Error::Unavailable)),
                Poll::Pending if driver.is_none() => Poll::Ready(Err(Error::Unavailable)),
                Poll::Pending => Poll::Pending,
            }
        })
        .await?;
        drop(sending);
        let (parts, incoming) = response.into_parts();
        Ok(Response::from_parts(
            parts,
            Body::new(driver, incoming, sender),
        ))
    }

    async fn connect(&self, host: &str) -> Result<TcpStream, Error> {
        #[cfg(test)]
        if let Some((expected, address)) = &self.endpoint {
            if host != expected {
                return Err(Error::InvalidMedia);
            }
            return TcpStream::connect(address)
                .await
                .map_err(|_| Error::Unavailable);
        }
        TcpStream::connect((host, 443))
            .await
            .map_err(|_| Error::Unavailable)
    }
}
