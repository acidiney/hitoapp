use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub struct OwnedTls {
    pub acceptor: TlsAcceptor,
    pub root_der: Vec<u8>,
}

pub struct OwnedAuthority {
    pub root_der: Vec<u8>,
    issuer: Issuer<'static, KeyPair>,
}

impl OwnedAuthority {
    pub fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let key = KeyPair::generate().expect("the owned TLS authority key must generate");
        let mut authority = CertificateParams::new(Vec::<String>::new())
            .expect("the owned TLS authority must configure");
        authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        authority.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let root = authority
            .self_signed(&key)
            .expect("the owned TLS authority must sign");
        Self {
            root_der: root.der().to_vec(),
            issuer: Issuer::new(authority, key),
        }
    }

    pub fn acceptor(&self, host: &str) -> TlsAcceptor {
        let key = KeyPair::generate().expect("the owned TLS endpoint key must generate");
        let mut leaf = CertificateParams::new(vec![host.to_owned()])
            .expect("the owned TLS endpoint must configure");
        leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let certificate = leaf
            .signed_by(&key, &self.issuer)
            .expect("the owned TLS endpoint must sign");
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .expect("the owned TLS endpoint must initialize");
        TlsAcceptor::from(Arc::new(config))
    }
}

impl Default for OwnedAuthority {
    fn default() -> Self {
        Self::new()
    }
}

pub fn issue(host: &str) -> OwnedTls {
    let authority = OwnedAuthority::new();
    OwnedTls {
        acceptor: authority.acceptor(host),
        root_der: authority.root_der,
    }
}
