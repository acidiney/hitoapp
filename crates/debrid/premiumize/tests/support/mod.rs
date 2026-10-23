use hito_testkit::tls_api;
use std::net::SocketAddr;

pub use hito_testkit::tls_api::Request;

pub mod logs;
pub mod reply;
pub use reply::Reply;

const API_HOST: &str = "www.premiumize.me";

pub struct Fixture(tls_api::Fixture);

impl Fixture {
    pub async fn start(replies: Vec<Vec<u8>>) -> Self {
        Self(tls_api::Fixture::start(API_HOST, replies).await)
    }

    pub async fn responses(replies: Vec<Reply>) -> Self {
        Self(tls_api::Fixture::responses(API_HOST, replies).await)
    }

    pub fn address(&self) -> SocketAddr {
        self.0.address()
    }

    pub fn ca_der(&self) -> &[u8] {
        self.0.ca_der()
    }

    pub async fn close(self) -> Vec<Request> {
        self.0.close().await
    }
}
