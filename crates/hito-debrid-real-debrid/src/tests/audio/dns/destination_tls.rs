use super::super::{
    protocol::{Reply, read_request, reply},
    tls::{API_HOST, Origin, Received},
};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::LazyConfigAcceptor;

pub(super) const WRONG_HOST: &str = "wrong-api.hito-fixture.test";
const MAX_CONNECTIONS: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Rejection {
    UntrustedRoot,
    WrongName,
}

#[derive(Default)]
struct State {
    accepted: usize,
    completed: usize,
    rejected: Vec<Rejection>,
    received: Vec<Received>,
    failed: bool,
    forced: bool,
}

pub(super) struct Report {
    pub accepted: usize,
    pub completed: usize,
    pub rejected: Vec<Rejection>,
    pub received: Vec<Received>,
    pub failed: bool,
    pub forced: bool,
    pub joined: bool,
}

pub(super) struct Fixture {
    pub address: SocketAddr,
    ca_der: Vec<u8>,
    state: Arc<Mutex<State>>,
    changed: Arc<Notify>,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    pub async fn start(rejection: Option<Rejection>, metadata: Vec<u8>) -> Self {
        assert!(metadata.len() <= 65_536);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (ca_der, trusted_issuer) = issuer("private-trusted-api-fixture-ca");
        let valid = configuration(API_HOST, &trusted_issuer);
        let invalid = rejection.map(|rejection| match rejection {
            Rejection::UntrustedRoot => {
                let (_, untrusted) = issuer("private-untrusted-api-fixture-ca");
                configuration(API_HOST, &untrusted)
            }
            Rejection::WrongName => configuration(WRONG_HOST, &trusted_issuer),
        });
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the owned rejecting API listener must bind");
        let address = listener
            .local_addr()
            .expect("the rejecting API listener must have an address");
        let state = Arc::new(Mutex::new(State::default()));
        let changed = Arc::new(Notify::new());
        let serving = state.clone();
        let notifications = changed.clone();
        let (stop, mut stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            let mut accepted = 0;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        if !matches!(result, Some(Ok(()))) {
                            serving.lock().expect("the rejecting TLS state must lock").failed = true;
                            break;
                        }
                    },
                    socket = listener.accept(), if accepted < MAX_CONNECTIONS => {
                        let Ok((socket, _)) = socket else {
                            serving.lock().expect("the rejecting TLS state must lock").failed = true;
                            break;
                        };
                        let expected = if accepted == 0 { rejection } else { None };
                        let tls = if expected.is_some() {
                            invalid.as_ref().expect("the rejected certificate must be configured").clone()
                        } else { valid.clone() };
                        accepted += 1;
                        serving.lock().expect("the rejecting TLS state must lock").accepted = accepted;
                        let state = serving.clone();
                        let changed = notifications.clone();
                        let metadata = metadata.clone();
                        handlers.spawn(async move {
                            let result = tokio::time::timeout(Duration::from_secs(5), exchange(socket, tls, expected, &metadata, &state, &changed)).await;
                            let mut state = state.lock().expect("the rejecting TLS state must lock");
                            if matches!(result, Ok(Ok(()))) { state.completed += 1; }
                            else { state.failed = true; }
                            changed.notify_waiters();
                        });
                    }
                }
            }
            let drained = tokio::time::timeout(Duration::from_secs(1), async {
                while let Some(result) = handlers.join_next().await {
                    if result.is_err() {
                        serving
                            .lock()
                            .expect("the rejecting TLS state must lock")
                            .failed = true;
                    }
                }
            })
            .await;
            if drained.is_err() {
                serving
                    .lock()
                    .expect("the rejecting TLS state must lock")
                    .forced = true;
                handlers.abort_all();
                while handlers.join_next().await.is_some() {}
            }
        });
        Self {
            address,
            ca_der,
            state,
            changed,
            stop: Some(stop),
            server: Some(server),
        }
    }

    pub fn ca_der(&self) -> &[u8] {
        &self.ca_der
    }

    pub fn counts(&self) -> (usize, usize) {
        let state = self
            .state
            .lock()
            .expect("the rejecting TLS state must lock");
        (state.accepted, state.received.len())
    }

    pub async fn rejected(&self, expected: Rejection) -> Result<(), &'static str> {
        loop {
            let changed = self.changed.notified();
            {
                let state = self
                    .state
                    .lock()
                    .expect("the rejecting TLS state must lock");
                if state.failed {
                    return Err(
                        "the owned TLS rejection must match its exact expected alert and peer closure",
                    );
                }
                if state.rejected.as_slice() == [expected] {
                    return Ok(());
                }
            }
            changed.await;
        }
    }

    pub async fn close(mut self) -> Report {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let joined = if let Some(mut server) = self.server.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut server).await {
                Ok(result) => result.is_ok(),
                Err(_) => {
                    server.abort();
                    let _ = server.await;
                    false
                }
            }
        } else {
            false
        };
        let mut state = self
            .state
            .lock()
            .expect("the rejecting TLS state must lock");
        Report {
            accepted: state.accepted,
            completed: state.completed,
            rejected: std::mem::take(&mut state.rejected),
            received: std::mem::take(&mut state.received),
            failed: state.failed,
            forced: state.forced,
            joined,
        }
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

async fn exchange(
    socket: TcpStream,
    tls: Arc<rustls::ServerConfig>,
    expected: Option<Rejection>,
    metadata: &[u8],
    state: &Mutex<State>,
    changed: &Notify,
) -> Result<(), ()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let hello = tokio::time::timeout_at(
        deadline,
        LazyConfigAcceptor::new(rustls::server::Acceptor::default(), socket),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    if hello.client_hello().server_name() != Some(API_HOST) {
        return Err(());
    }
    let result = tokio::time::timeout_at(deadline, hello.into_stream(tls).into_fallible())
        .await
        .map_err(|_| ())?;
    let mut socket = match (expected, result) {
        (None, Ok(socket)) => socket,
        (Some(expected), Err((error, mut socket))) => {
            let alert = match expected {
                Rejection::UntrustedRoot => rustls::AlertDescription::UnknownCA,
                Rejection::WrongName => rustls::AlertDescription::BadCertificate,
            };
            if error.kind() != std::io::ErrorKind::InvalidData
                || !matches!(error.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()),
                    Some(rustls::Error::AlertReceived(received)) if *received == alert)
            {
                return Err(());
            }
            let mut byte = [0_u8; 1];
            let closed = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte)).await;
            if !matches!(closed, Ok(Ok(0))) {
                return Err(());
            }
            state.lock().map_err(|_| ())?.rejected.push(expected);
            changed.notify_waiters();
            return Ok(());
        }
        _ => return Err(()),
    };
    let server_name = socket
        .get_ref()
        .1
        .server_name()
        .unwrap_or_default()
        .to_owned();
    let request = read_request(&mut socket).await;
    {
        let mut state = state.lock().map_err(|_| ())?;
        if state.received.len() == MAX_CONNECTIONS {
            return Err(());
        }
        state.received.push(Received {
            origin: Origin::Api,
            server_name,
            request,
        });
    }
    reply(&mut socket, Reply::Json(metadata)).await;
    Ok(())
}

fn issuer(name: &str) -> (Vec<u8>, Issuer<'static, KeyPair>) {
    let key = KeyPair::generate().expect("the owned API fixture CA key must generate");
    let mut params =
        CertificateParams::new(Vec::<String>::new()).expect("the CA parameters must initialize");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.distinguished_name.push(DnType::CommonName, name);
    let certificate = params
        .self_signed(&key)
        .expect("the owned API fixture CA must sign");
    (certificate.der().to_vec(), Issuer::new(params, key))
}

fn configuration(host: &str, issuer: &Issuer<'_, KeyPair>) -> Arc<rustls::ServerConfig> {
    let key = KeyPair::generate().expect("the owned API certificate key must generate");
    let mut params =
        CertificateParams::new(vec![host.into()]).expect("the leaf parameters must initialize");
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let certificate = params
        .signed_by(&key, issuer)
        .expect("the owned API certificate must sign");
    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .expect("the owned API TLS configuration must initialize"),
    )
}
