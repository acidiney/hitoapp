use super::protocol::{FramedReply, Reply, Request, framed_reply, read_request, reply};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::TlsAcceptor;

pub(super) const API_HOST: &str = "api.real-debrid.com";
pub(super) const MEDIA_HOST: &str = "media.hito-fixture.invalid";
pub(super) const DNS_MEDIA_HOST: &str = "media.hito-fixture.test";
const MAX_CONNECTIONS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Api,
    Media,
}

pub(super) struct Received {
    pub origin: Origin,
    pub server_name: String,
    pub request: Request,
}

struct State {
    api_replies: Mutex<VecDeque<Vec<u8>>>,
    audio: MediaReply,
    received: Mutex<Vec<Received>>,
}

enum MediaReply {
    Wav(Vec<u8>),
    Framed(FramedReply),
}

pub(super) struct Fixture {
    ca_der: Vec<u8>,
    pub api_address: SocketAddr,
    pub media_address: SocketAddr,
    state: Arc<State>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    pub async fn start(api_replies: Vec<Vec<u8>>, audio: Vec<u8>) -> Self {
        assert!(audio.len() <= 1_048_576);
        Self::configured(api_replies, MediaReply::Wav(audio), MEDIA_HOST).await
    }

    pub async fn with_media_reply(api_replies: Vec<Vec<u8>>, media: FramedReply) -> Self {
        media.validate();
        Self::configured(api_replies, MediaReply::Framed(media), MEDIA_HOST).await
    }

    pub async fn with_dns_media(api_replies: Vec<Vec<u8>>, audio: Vec<u8>) -> Self {
        assert!(audio.len() <= 1_048_576);
        Self::configured(api_replies, MediaReply::Wav(audio), DNS_MEDIA_HOST).await
    }

    async fn configured(
        api_replies: Vec<Vec<u8>>,
        audio: MediaReply,
        media_host: &'static str,
    ) -> Self {
        assert!(
            api_replies.len() <= MAX_CONNECTIONS
                && api_replies.iter().all(|body| body.len() <= 65_536)
        );
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca_key = KeyPair::generate().expect("the owned TLS CA key must generate");
        let mut ca = CertificateParams::new(Vec::<String>::new())
            .expect("the owned CA parameters must initialize");
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let root = ca.self_signed(&ca_key).expect("the owned TLS CA must sign");
        let ca_der = root.der().to_vec();
        let issuer = Issuer::new(ca, ca_key);
        let api_tls = acceptor(API_HOST, &issuer);
        let media_tls = acceptor(media_host, &issuer);
        let api = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned API TLS listener must bind");
        let media = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned media TLS listener must bind");
        let api_address = api
            .local_addr()
            .expect("the owned API listener must have an address");
        let media_address = media
            .local_addr()
            .expect("the owned media listener must have an address");
        let state = Arc::new(State {
            api_replies: Mutex::new(api_replies.into()),
            audio,
            received: Mutex::new(Vec::new()),
        });
        let serving = state.clone();
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            let mut accepted_count = 0;
            let mut failed = false;
            loop {
                let (socket, origin, tls) = tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        if !matches!(result, Some(Ok(()))) { failed = true; break; }
                        continue;
                    },
                    accepted = api.accept(), if accepted_count < MAX_CONNECTIONS => {
                        let Ok((socket, _)) = accepted else { failed = true; break; };
                        (socket, Origin::Api, api_tls.clone())
                    },
                    accepted = media.accept(), if accepted_count < MAX_CONNECTIONS => {
                        let Ok((socket, _)) = accepted else { failed = true; break; };
                        (socket, Origin::Media, media_tls.clone())
                    },
                };
                accepted_count += 1;
                let serving = serving.clone();
                handlers.spawn(async move {
                    tokio::time::timeout(
                        Duration::from_secs(5),
                        exchange(socket, origin, tls, serving),
                    )
                    .await
                    .expect("the owned TLS exchange must finish within its bound");
                });
            }
            handlers.abort_all();
            while let Some(result) = handlers.join_next().await {
                if result.is_err_and(|error| !error.is_cancelled()) {
                    failed = true;
                }
            }
            assert!(
                !failed,
                "an owned TLS handler failed before joined shutdown"
            );
        });
        Self {
            ca_der,
            api_address,
            media_address,
            state,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub fn ca_der(&self) -> &[u8] {
        &self.ca_der
    }

    pub async fn close(mut self) -> Vec<Received> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.expect("the owned TLS fixture must stop"),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    panic!("the owned TLS fixture did not join shutdown");
                }
            }
        }
        let mut received = self
            .state
            .received
            .lock()
            .expect("the TLS request capture must lock");
        std::mem::take(&mut *received)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

fn acceptor(host: &str, issuer: &Issuer<'_, KeyPair>) -> TlsAcceptor {
    let key = KeyPair::generate().expect("the owned TLS server key must generate");
    let mut leaf = CertificateParams::new(vec![host.into()])
        .expect("the owned TLS server parameters must initialize");
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let certificate = leaf
        .signed_by(&key, issuer)
        .expect("the owned TLS server certificate must sign");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .expect("the owned TLS server must initialize");
    TlsAcceptor::from(Arc::new(config))
}

async fn exchange(socket: TcpStream, origin: Origin, tls: TlsAcceptor, state: Arc<State>) {
    let mut socket = tokio::time::timeout(Duration::from_secs(2), tls.accept(socket))
        .await
        .expect("the owned TLS handshake must finish")
        .expect("the owned TLS handshake must succeed");
    let server_name = socket
        .get_ref()
        .1
        .server_name()
        .unwrap_or_default()
        .to_owned();
    let request = read_request(&mut socket).await;
    {
        let mut received = state
            .received
            .lock()
            .expect("the TLS request capture must lock");
        assert!(
            received.len() < MAX_CONNECTIONS,
            "the TLS request count exceeded its bound"
        );
        received.push(Received {
            origin,
            server_name,
            request,
        });
    }
    if origin == Origin::Media {
        match &state.audio {
            MediaReply::Wav(audio) => reply(&mut socket, Reply::Wav(audio)).await,
            MediaReply::Framed(response) => framed_reply(&mut socket, response).await,
        }
    } else {
        let body = state
            .api_replies
            .lock()
            .expect("the owned API reply queue must lock")
            .pop_front();
        match body {
            Some(body) => reply(&mut socket, Reply::Json(&body)).await,
            None => reply(&mut socket, Reply::Rejected).await,
        }
    }
}
