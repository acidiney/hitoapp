use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub struct OwnedTls {
    pub acceptor: TlsAcceptor,
    pub root_der: Vec<u8>,
}

pub fn issue(hosts: &[&str]) -> OwnedTls {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let authority_key = KeyPair::generate().expect("the owned TLS authority key must generate");
    let mut authority = CertificateParams::new(Vec::<String>::new())
        .expect("the owned TLS authority must configure");
    authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    authority.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    let root = authority
        .self_signed(&authority_key)
        .expect("the owned TLS authority must sign");
    let root_der = root.der().to_vec();
    let issuer = Issuer::new(authority, authority_key);
    let key = KeyPair::generate().expect("the owned TLS endpoint key must generate");
    let certificate = CertificateParams::new(
        hosts
            .iter()
            .map(|host| (*host).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("the owned TLS endpoint must configure")
    .signed_by(&key, &issuer)
    .expect("the owned TLS endpoint must sign");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        )
        .expect("the owned TLS endpoint must initialize");
    OwnedTls {
        acceptor: TlsAcceptor::from(Arc::new(config)),
        root_der,
    }
}
